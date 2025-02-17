// Copyright (C) 2021-2022 Selendra.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Common logic for implementation of worker processes.

use crate::LOG_TARGET;
use async_std::{
	io,
	os::unix::net::{UnixListener, UnixStream},
	path::{Path, PathBuf},
};
use futures::{
	never::Never, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, FutureExt as _,
};
use futures_timer::Delay;
use pin_project::pin_project;
use rand::Rng;
use std::{
	fmt, mem,
	pin::Pin,
	task::{Context, Poll},
	time::Duration,
};

/// This is publicly exposed only for integration tests.
#[doc(hidden)]
pub async fn spawn_with_program_path(
	debug_id: &'static str,
	program_path: impl Into<PathBuf>,
	extra_args: &'static [&'static str],
	spawn_timeout: Duration,
) -> Result<(IdleWorker, WorkerHandle), SpawnErr> {
	let program_path = program_path.into();
	with_transient_socket_path(debug_id, |socket_path| {
		let socket_path = socket_path.to_owned();
		async move {
			let listener = UnixListener::bind(&socket_path).await.map_err(|err| {
				gum::warn!(
					target: LOG_TARGET,
					%debug_id,
					"cannot bind unix socket: {:?}",
					err,
				);
				SpawnErr::Bind
			})?;

			let handle =
				WorkerHandle::spawn(program_path, extra_args, socket_path).map_err(|err| {
					gum::warn!(
						target: LOG_TARGET,
						%debug_id,
						"cannot spawn a worker: {:?}",
						err,
					);
					SpawnErr::ProcessSpawn
				})?;

			futures::select! {
				accept_result = listener.accept().fuse() => {
					let (stream, _) = accept_result.map_err(|err| {
						gum::warn!(
							target: LOG_TARGET,
							%debug_id,
							"cannot accept a worker: {:?}",
							err,
						);
						SpawnErr::Accept
					})?;
					Ok((IdleWorker { stream, pid: handle.id() }, handle))
				}
				_ = Delay::new(spawn_timeout).fuse() => {
					Err(SpawnErr::AcceptTimeout)
				}
			}
		}
	})
	.await
}

async fn with_transient_socket_path<T, F, Fut>(debug_id: &'static str, f: F) -> Result<T, SpawnErr>
where
	F: FnOnce(&Path) -> Fut,
	Fut: futures::Future<Output = Result<T, SpawnErr>> + 'static,
{
	let socket_path = tmpfile(&format!("pvf-host-{}", debug_id))
		.await
		.map_err(|_| SpawnErr::TmpFile)?;
	let result = f(&socket_path).await;

	// Best effort to remove the socket file. Under normal circumstances the socket will be removed
	// by the worker. We make sure that it is removed here, just in case a failed rendezvous.
	let _ = async_std::fs::remove_file(socket_path).await;

	result
}

/// Returns a path under the given `dir`. The file name will start with the given prefix.
///
/// There is only a certain number of retries. If exceeded this function will give up and return an
/// error.
pub async fn tmpfile_in(prefix: &str, dir: &Path) -> io::Result<PathBuf> {
	fn tmppath(prefix: &str, dir: &Path) -> PathBuf {
		use rand::distributions::Alphanumeric;

		const DESCRIMINATOR_LEN: usize = 10;

		let mut buf = Vec::with_capacity(prefix.len() + DESCRIMINATOR_LEN);
		buf.extend(prefix.as_bytes());
		buf.extend(rand::thread_rng().sample_iter(&Alphanumeric).take(DESCRIMINATOR_LEN));

		let s = std::str::from_utf8(&buf)
			.expect("the string is collected from a valid utf-8 sequence; qed");

		let mut file = dir.to_owned();
		file.push(s);
		file
	}

	const NUM_RETRIES: usize = 50;

	for _ in 0..NUM_RETRIES {
		let candidate_path = tmppath(prefix, dir);
		if !candidate_path.exists().await {
			return Ok(candidate_path)
		}
	}

	Err(io::Error::new(io::ErrorKind::Other, "failed to create a temporary file"))
}

/// The same as [`tmpfile_in`], but uses [`std::env::temp_dir`] as the directory.
pub async fn tmpfile(prefix: &str) -> io::Result<PathBuf> {
	let temp_dir = PathBuf::from(std::env::temp_dir());
	tmpfile_in(prefix, &temp_dir).await
}

pub fn worker_event_loop<F, Fut>(debug_id: &'static str, socket_path: &str, mut event_loop: F)
where
	F: FnMut(UnixStream) -> Fut,
	Fut: futures::Future<Output = io::Result<Never>>,
{
	let err = async_std::task::block_on::<_, io::Result<Never>>(async move {
		let stream = UnixStream::connect(socket_path).await?;
		let _ = async_std::fs::remove_file(socket_path).await;

		event_loop(stream).await
	})
	.unwrap_err(); // it's never `Ok` because it's `Ok(Never)`

	gum::debug!(
		target: LOG_TARGET,
		worker_pid = %std::process::id(),
		"pvf worker ({}): {:?}",
		debug_id,
		err,
	);
}

/// A struct that represents an idle worker.
///
/// This struct is supposed to be used as a token that is passed by move into a subroutine that
/// initiates a job. If the worker dies on the duty, then the token is not returned.
#[derive(Debug)]
pub struct IdleWorker {
	/// The stream to which the child process is connected.
	pub stream: UnixStream,

	/// The identifier of this process. Used to reset the niceness.
	pub pid: u32,
}

/// An error happened during spawning a worker process.
#[derive(Clone, Debug)]
pub enum SpawnErr {
	/// Cannot obtain a temporary file location.
	TmpFile,
	/// Cannot bind the socket to the given path.
	Bind,
	/// An error happened during accepting a connection to the socket.
	Accept,
	/// An error happened during spawning the process.
	ProcessSpawn,
	/// The deadline allotted for the worker spawning and connecting to the socket has elapsed.
	AcceptTimeout,
}

/// This is a representation of a potentially running worker. Drop it and the process will be killed.
///
/// A worker's handle is also a future that resolves when it's detected that the worker's process
/// has been terminated. Since the worker is running in another process it is obviously not necessarily
///  to poll this future to make the worker run, it's only for termination detection.
///
/// This future relies on the fact that a child process's stdout `fd` is closed upon it's termination.
#[pin_project]
pub struct WorkerHandle {
	child: async_process::Child,
	#[pin]
	stdout: async_process::ChildStdout,
	drop_box: Box<[u8]>,
}

impl WorkerHandle {
	fn spawn(
		program: impl AsRef<Path>,
		extra_args: &[&str],
		socket_path: impl AsRef<Path>,
	) -> io::Result<Self> {
		let mut child = async_process::Command::new(program.as_ref())
			.args(extra_args)
			.arg(socket_path.as_ref().as_os_str())
			.stdout(async_process::Stdio::piped())
			.kill_on_drop(true)
			.spawn()?;

		let stdout = child
			.stdout
			.take()
			.expect("the process spawned with piped stdout should have the stdout handle");

		Ok(WorkerHandle {
			child,
			stdout,
			// We don't expect the bytes to be ever read. But in case we do, we should not use a buffer
			// of a small size, because otherwise if the child process does return any data we will end up
			// issuing a syscall for each byte. We also prefer not to do allocate that on the stack, since
			// each poll the buffer will be allocated and initialized (and that's due `poll_read` takes &mut [u8]
			// and there are no guarantees that a `poll_read` won't ever read from there even though that's
			// unlikely).
			//
			// OTOH, we also don't want to be super smart here and we could just afford to allocate a buffer
			// for that here.
			drop_box: vec![0; 8192].into_boxed_slice(),
		})
	}

	/// Returns the process id of this worker.
	pub fn id(&self) -> u32 {
		self.child.id()
	}
}

impl futures::Future for WorkerHandle {
	type Output = ();

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let me = self.project();
		match futures::ready!(AsyncRead::poll_read(me.stdout, cx, &mut *me.drop_box)) {
			Ok(0) => {
				// 0 means `EOF` means the child was terminated. Resolve.
				Poll::Ready(())
			},
			Ok(_bytes_read) => {
				// weird, we've read something. Pretend that never happened and reschedule ourselves.
				cx.waker().wake_by_ref();
				Poll::Pending
			},
			Err(_) => {
				// The implementation is guaranteed to not to return `WouldBlock` and Interrupted. This
				// leaves us with a legit errors which we suppose were due to termination.
				Poll::Ready(())
			},
		}
	}
}

impl fmt::Debug for WorkerHandle {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "WorkerHandle(pid={})", self.id())
	}
}

/// Convert the given path into a byte buffer.
pub fn path_to_bytes(path: &Path) -> &[u8] {
	// Ideally, we take the `OsStr` of the path, send that and reconstruct this on the other side.
	// However, libstd doesn't provide us with such an option. There are crates out there that
	// allow for extraction of a path, but TBH it doesn't seem to be a real issue.
	//
	// However, should be there reports we can incorporate such a crate here.
	path.to_str().expect("non-UTF-8 path").as_bytes()
}

/// Interprets the given bytes as a path. Returns `None` if the given bytes do not constitute a
/// a proper utf-8 string.
pub fn bytes_to_path(bytes: &[u8]) -> Option<PathBuf> {
	std::str::from_utf8(bytes).ok().map(PathBuf::from)
}

pub async fn framed_send(w: &mut (impl AsyncWrite + Unpin), buf: &[u8]) -> io::Result<()> {
	let len_buf = buf.len().to_le_bytes();
	w.write_all(&len_buf).await?;
	w.write_all(buf).await?;
	Ok(())
}

pub async fn framed_recv(r: &mut (impl AsyncRead + Unpin)) -> io::Result<Vec<u8>> {
	let mut len_buf = [0u8; mem::size_of::<usize>()];
	r.read_exact(&mut len_buf).await?;
	let len = usize::from_le_bytes(len_buf);
	let mut buf = vec![0; len];
	r.read_exact(&mut buf).await?;
	Ok(buf)
}

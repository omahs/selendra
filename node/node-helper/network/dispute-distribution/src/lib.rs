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

//! # Sending and receiving of `DisputeRequest`s.
//!
//! This subsystem essentially consists of two parts:
//!
//! - a sender
//! - and a receiver
//!
//! The sender is responsible for getting our vote out, see [`sender`]. The receiver handles
//! incoming [`DisputeRequest`]s and offers spam protection, see [`receiver`].

use futures::{channel::mpsc, FutureExt, StreamExt, TryFutureExt};

use selendra_node_network_protocol::authority_discovery::AuthorityDiscovery;
use sp_keystore::SyncCryptoStorePtr;

use selendra_node_network_protocol::request_response::{incoming::IncomingRequestReceiver, v1};
use selendra_node_primitives::DISPUTE_WINDOW;
use selendra_node_subsystem::{
	messages::DisputeDistributionMessage, overseer, FromOrchestra, OverseerSignal,
	SpawnedSubsystem, SubsystemError,
};
use selendra_node_subsystem_util::{runtime, runtime::RuntimeInfo};

/// ## The sender [`DisputeSender`]
///
/// The sender (`DisputeSender`) keeps track of live disputes and makes sure our vote gets out for
/// each one of those. The sender is responsible for sending our vote to each validator
/// participating in the dispute and to each authority currently authoring blocks. The sending can
/// be initiated by sending `DisputeDistributionMessage::SendDispute` message to this subsystem.
///
/// In addition the `DisputeSender` will query the coordinator for active disputes on each
/// [`DisputeSender::update_leaves`] call and will initiate sending (start a `SendTask`) for every,
/// to this subsystem, unknown dispute. This is to make sure, we get our vote out, even on
/// restarts.
///
///	The actual work of sending and keeping track of transmission attempts to each validator for a
///	particular dispute are done by [`SendTask`].  The purpose of the `DisputeSender` is to keep
///	track of all ongoing disputes and start and clean up `SendTask`s accordingly.
mod sender;
use self::sender::{DisputeSender, TaskFinish};

///	## The receiver [`DisputesReceiver`]
///
///	The receiving side is implemented as `DisputesReceiver` and is run as a separate long running task within
///	this subsystem ([`DisputesReceiver::run`]).
///
///	Conceptually all the receiver has to do, is waiting for incoming requests which are passed in
///	via a dedicated channel and forwarding them to the dispute coordinator via
///	`DisputeCoordinatorMessage::ImportStatements`. Being the interface to the network and untrusted
///	nodes, the reality is not that simple of course. Before importing statements the receiver will
///	make sure as good as it can to filter out malicious/unwanted/spammy requests. For this it does
///	the following:
///
///	- Drop all messages from non validator nodes, for this it requires the [`AuthorityDiscovery`]
///	service.
///	- Drop messages from a node, if we are already importing a message from that node (flood).
///	- Drop messages from nodes, that provided us messages where the statement import failed.
///	- Drop any obviously invalid votes (invalid signatures for example).
///	- Ban peers whose votes were deemed invalid.
///
/// For successfully imported votes, we will confirm the receipt of the message back to the sender.
/// This way a received confirmation guarantees, that the vote has been stored to disk by the
/// receiver.
mod receiver;
use self::receiver::DisputesReceiver;

/// Error and [`Result`] type for this subsystem.
mod error;
use error::{log_error, FatalError, FatalResult, Result};

#[cfg(test)]
mod tests;

mod metrics;
//// Prometheus `Metrics` for dispute distribution.
pub use metrics::Metrics;

const LOG_TARGET: &'static str = "parachain::dispute-distribution";

/// The dispute distribution subsystem.
pub struct DisputeDistributionSubsystem<AD> {
	/// Easy and efficient runtime access for this subsystem.
	runtime: RuntimeInfo,

	/// Sender for our dispute requests.
	disputes_sender: DisputeSender,

	/// Receive messages from `SendTask`.
	sender_rx: mpsc::Receiver<TaskFinish>,

	/// Receiver for incoming requests.
	req_receiver: Option<IncomingRequestReceiver<v1::DisputeRequest>>,

	/// Authority discovery service.
	authority_discovery: AD,

	/// Metrics for this subsystem.
	metrics: Metrics,
}

#[overseer::subsystem(DisputeDistribution, error = SubsystemError, prefix = self::overseer)]
impl<Context, AD> DisputeDistributionSubsystem<AD>
where
	<Context as overseer::DisputeDistributionContextTrait>::Sender:
		overseer::DisputeDistributionSenderTrait + Sync + Send,
	AD: AuthorityDiscovery + Clone,
{
	fn start(self, ctx: Context) -> SpawnedSubsystem {
		let future = self
			.run(ctx)
			.map_err(|e| SubsystemError::with_origin("dispute-distribution", e))
			.boxed();

		SpawnedSubsystem { name: "dispute-distribution-subsystem", future }
	}
}

#[overseer::contextbounds(DisputeDistribution, prefix = self::overseer)]
impl<AD> DisputeDistributionSubsystem<AD>
where
	AD: AuthorityDiscovery + Clone,
{
	/// Create a new instance of the availability distribution.
	pub fn new(
		keystore: SyncCryptoStorePtr,
		req_receiver: IncomingRequestReceiver<v1::DisputeRequest>,
		authority_discovery: AD,
		metrics: Metrics,
	) -> Self {
		let runtime = RuntimeInfo::new_with_config(runtime::Config {
			keystore: Some(keystore),
			session_cache_lru_size: DISPUTE_WINDOW.get() as usize,
		});
		let (tx, sender_rx) = mpsc::channel(1);
		let disputes_sender = DisputeSender::new(tx, metrics.clone());
		Self {
			runtime,
			disputes_sender,
			sender_rx,
			req_receiver: Some(req_receiver),
			authority_discovery,
			metrics,
		}
	}

	/// Start processing work as passed on from the Overseer.
	async fn run<Context>(mut self, mut ctx: Context) -> std::result::Result<(), FatalError> {
		let receiver = DisputesReceiver::new(
			ctx.sender().clone(),
			self.req_receiver
				.take()
				.expect("Must be provided on `new` and we take ownership here. qed."),
			self.authority_discovery.clone(),
			self.metrics.clone(),
		);
		ctx.spawn("disputes-receiver", receiver.run().boxed())
			.map_err(FatalError::SpawnTask)?;

		loop {
			let message = MuxedMessage::receive(&mut ctx, &mut self.sender_rx).await;
			match message {
				MuxedMessage::Subsystem(result) => {
					let result = match result? {
						FromOrchestra::Signal(signal) => {
							match self.handle_signals(&mut ctx, signal).await {
								Ok(SignalResult::Conclude) => return Ok(()),
								Ok(SignalResult::Continue) => Ok(()),
								Err(f) => Err(f),
							}
						},
						FromOrchestra::Communication { msg } =>
							self.handle_subsystem_message(&mut ctx, msg).await,
					};
					log_error(result, "on FromOrchestra")?;
				},
				MuxedMessage::Sender(result) => {
					self.disputes_sender
						.on_task_message(result.ok_or(FatalError::SenderExhausted)?)
						.await;
				},
			}
		}
	}

	/// Handle overseer signals.
	async fn handle_signals<Context>(
		&mut self,
		ctx: &mut Context,
		signal: OverseerSignal,
	) -> Result<SignalResult> {
		match signal {
			OverseerSignal::Conclude => return Ok(SignalResult::Conclude),
			OverseerSignal::ActiveLeaves(update) => {
				self.disputes_sender.update_leaves(ctx, &mut self.runtime, update).await?;
			},
			OverseerSignal::BlockFinalized(_, _) => {},
		};
		Ok(SignalResult::Continue)
	}

	/// Handle `DisputeDistributionMessage`s.
	async fn handle_subsystem_message<Context>(
		&mut self,
		ctx: &mut Context,
		msg: DisputeDistributionMessage,
	) -> Result<()> {
		match msg {
			DisputeDistributionMessage::SendDispute(dispute_msg) =>
				self.disputes_sender.start_sender(ctx, &mut self.runtime, dispute_msg).await?,
		}
		Ok(())
	}
}

/// Messages to be handled in this subsystem.
#[derive(Debug)]
enum MuxedMessage {
	/// Messages from other subsystems.
	Subsystem(FatalResult<FromOrchestra<DisputeDistributionMessage>>),
	/// Messages from spawned sender background tasks.
	Sender(Option<TaskFinish>),
}

#[overseer::contextbounds(DisputeDistribution, prefix = self::overseer)]
impl MuxedMessage {
	async fn receive<Context>(
		ctx: &mut Context,
		from_sender: &mut mpsc::Receiver<TaskFinish>,
	) -> Self {
		// We are only fusing here to make `select` happy, in reality we will quit if the stream
		// ends.
		let from_overseer = ctx.recv().fuse();
		futures::pin_mut!(from_overseer, from_sender);
		futures::select!(
			msg = from_overseer => MuxedMessage::Subsystem(msg.map_err(FatalError::SubsystemReceive)),
			msg = from_sender.next() => MuxedMessage::Sender(msg),
		)
	}
}

/// Result of handling signal from overseer.
enum SignalResult {
	/// Overseer asked us to conclude.
	Conclude,
	/// We can continue processing events.
	Continue,
}

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

use sp_runtime::traits::Block as BlockT;

use selendra_node_primitives::AvailableData;
use selendra_node_subsystem::messages::AvailabilityRecoveryMessage;
use selendra_overseer::Handle as OverseerHandle;

use futures::{channel::oneshot, stream::FuturesUnordered, Future, FutureExt, StreamExt};

use std::{collections::HashSet, pin::Pin};

/// The active candidate recovery.
///
/// This handles the candidate recovery and tracks the activate recoveries.
pub(crate) struct ActiveCandidateRecovery<Block: BlockT> {
	/// The recoveries that are currently being executed.
	recoveries: FuturesUnordered<
		Pin<Box<dyn Future<Output = (Block::Hash, Option<AvailableData>)> + Send>>,
	>,
	/// The block hashes of the candidates currently being recovered.
	candidates: HashSet<Block::Hash>,
	overseer_handle: OverseerHandle,
}

impl<Block: BlockT> ActiveCandidateRecovery<Block> {
	pub fn new(overseer_handle: OverseerHandle) -> Self {
		Self { recoveries: Default::default(), candidates: Default::default(), overseer_handle }
	}

	/// Recover the given `pending_candidate`.
	pub async fn recover_candidate(
		&mut self,
		block_hash: Block::Hash,
		pending_candidate: crate::PendingCandidate<Block>,
	) {
		let (tx, rx) = oneshot::channel();

		self.overseer_handle
			.send_msg(
				AvailabilityRecoveryMessage::RecoverAvailableData(
					pending_candidate.receipt,
					pending_candidate.session_index,
					None,
					tx,
				),
				"ActiveCandidateRecovery",
			)
			.await;

		self.candidates.insert(block_hash);

		self.recoveries.push(
			async move {
				match rx.await {
					Ok(Ok(res)) => (block_hash, Some(res)),
					Ok(Err(error)) => {
						tracing::debug!(
							target: crate::LOG_TARGET,
							?error,
							?block_hash,
							"Availability recovery failed",
						);
						(block_hash, None)
					},
					Err(_) => {
						tracing::debug!(
							target: crate::LOG_TARGET,
							"Availability recovery oneshot channel closed",
						);
						(block_hash, None)
					},
				}
			}
			.boxed(),
		);
	}

	/// Returns if the given `candidate` is being recovered.
	pub fn is_being_recovered(&self, candidate: &Block::Hash) -> bool {
		self.candidates.contains(candidate)
	}

	/// Waits for the next recovery.
	///
	/// If the returned [`AvailableData`] is `None`, it means that the recovery failed.
	pub async fn wait_for_recovery(&mut self) -> (Block::Hash, Option<AvailableData>) {
		loop {
			if let Some(res) = self.recoveries.next().await {
				self.candidates.remove(&res.0);
				return res
			} else {
				futures::pending!()
			}
		}
	}
}

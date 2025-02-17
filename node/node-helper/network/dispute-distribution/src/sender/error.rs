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
//

//! Error handling related code and Error/Result definitions.

use selendra_node_primitives::disputes::DisputeMessageCheckError;
use selendra_node_subsystem::SubsystemError;
use selendra_node_subsystem_util::runtime;

#[allow(missing_docs)]
#[fatality::fatality(splitable)]
pub enum Error {
	#[fatal]
	#[error("Spawning subsystem task failed")]
	SpawnTask(#[source] SubsystemError),

	#[fatal(forward)]
	#[error("Error while accessing runtime information")]
	Runtime(#[from] runtime::Error),

	/// We need available active heads for finding relevant authorities.
	#[error("No active heads available - needed for finding relevant authorities.")]
	NoActiveHeads,

	/// This error likely indicates a bug in the coordinator.
	#[error("Oneshot for asking dispute coordinator for active disputes got canceled.")]
	AskActiveDisputesCanceled,

	/// This error likely indicates a bug in the coordinator.
	#[error("Oneshot for asking dispute coordinator for candidate votes got canceled.")]
	AskCandidateVotesCanceled,

	/// This error does indicate a bug in the coordinator.
	///
	/// We were not able to successfully construct a `DisputeMessage` from disputes votes.
	#[error("Invalid dispute encountered")]
	InvalidDisputeFromCoordinator(#[source] DisputeMessageCheckError),

	/// This error does indicate a bug in the coordinator.
	///
	/// We did not receive votes on both sides for `CandidateVotes` received from the coordinator.
	#[error("Missing votes for valid dispute")]
	MissingVotesFromCoordinator,

	/// This error does indicate a bug in the coordinator.
	///
	/// `SignedDisputeStatement` could not be reconstructed from recorded statements.
	#[error("Invalid statements from coordinator")]
	InvalidStatementFromCoordinator,

	/// This error does indicate a bug in the coordinator.
	///
	/// A statement's `ValidatorIndex` could not be looked up.
	#[error("ValidatorIndex of statement could not be found")]
	InvalidValidatorIndexFromCoordinator,
}

pub type Result<T> = std::result::Result<T, Error>;
pub type JfyiErrorResult<T> = std::result::Result<T, JfyiError>;

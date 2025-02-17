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

//! This module focuses on the benchmarking of the `include_pvf_check_statement` dispatchable.

use crate::{configuration, paras::*, shared::Pallet as ParasShared};
use frame_system::RawOrigin;
use primitives::v2::{HeadData, Id as ParaId, ValidationCode, ValidatorId, ValidatorIndex};
use sp_application_crypto::RuntimeAppPublic;

// Constants for the benchmarking
const SESSION_INDEX: SessionIndex = 1;
const VALIDATOR_NUM: usize = 800;
const CAUSES_NUM: usize = 100;
fn validation_code() -> ValidationCode {
	ValidationCode(vec![0])
}
fn old_validation_code() -> ValidationCode {
	ValidationCode(vec![1])
}

/// Prepares the PVF check statement and the validator signature to pass into
/// `include_pvf_check_statement` during benchmarking phase.
///
/// It won't trigger finalization, so we expect the benchmarking will only measure the performance
/// of only vote accounting.
pub fn prepare_inclusion_bench<T>() -> (PvfCheckStatement, ValidatorSignature)
where
	T: Config + shared::Config,
{
	initialize::<T>();
	// we do not plan to trigger finalization, thus the cause is inconsequential.
	initialize_pvf_active_vote::<T>(VoteCause::Onboarding);

	// `unwrap` cannot panic here since the `initialize` function should initialize validators count
	// to be more than 0.
	//
	// VoteDirection doesn't matter here as well.
	let stmt_n_sig = generate_statements::<T>(VoteOutcome::Accept).next().unwrap();

	stmt_n_sig
}

/// Prepares conditions for benchmarking of the finalization part of `include_pvf_check_statement`.
///
/// This function will initialize a PVF pre-check vote, then submit a number of PVF pre-checking
/// statements so that to achieve the quorum only one statement is left. This statement is returned
/// from this function and is expected to be passed into `include_pvf_check_statement` during the
/// benchmarking phase.
pub fn prepare_finalization_bench<T>(
	cause: VoteCause,
	outcome: VoteOutcome,
) -> (PvfCheckStatement, ValidatorSignature)
where
	T: Config + shared::Config,
{
	initialize::<T>();
	initialize_pvf_active_vote::<T>(cause);

	let mut stmts = generate_statements::<T>(outcome).collect::<Vec<_>>();
	// this should be ensured by the `initialize` function.
	assert!(stmts.len() > 2);

	// stash the last statement to be used in the benchmarking phase.
	let stmt_n_sig = stmts.pop().unwrap();

	for (stmt, sig) in stmts {
		let r = Pallet::<T>::include_pvf_check_statement(RawOrigin::None.into(), stmt, sig);
		assert!(r.is_ok());
	}

	stmt_n_sig
}

/// What caused the PVF pre-checking vote?
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum VoteCause {
	Onboarding,
	Upgrade,
}

/// The outcome of the PVF pre-checking vote.
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum VoteOutcome {
	Accept,
	Reject,
}

fn initialize<T>()
where
	T: Config + shared::Config,
{
	// 0. generate a list of validators
	let validators = (0..VALIDATOR_NUM)
		.map(|_| <ValidatorId as RuntimeAppPublic>::generate_pair(None))
		.collect::<Vec<_>>();

	// 1. Make sure PVF pre-checking is enabled in the config.
	let mut config = configuration::Pallet::<T>::config();
	config.pvf_checking_enabled = true;
	configuration::Pallet::<T>::force_set_active_config(config.clone());

	// 2. initialize a new session with deterministic validator set.
	ParasShared::<T>::set_active_validators_ascending(validators.clone());
	ParasShared::<T>::set_session_index(SESSION_INDEX);
}

/// Creates a new PVF pre-checking active vote.
///
/// The subject of the vote (i.e. validation code) and the cause (upgrade/onboarding) is specified
/// by the test setup.
fn initialize_pvf_active_vote<T>(vote_cause: VoteCause)
where
	T: Config + shared::Config,
{
	for i in 0..CAUSES_NUM {
		let id = ParaId::from(i as u32);

		if vote_cause == VoteCause::Upgrade {
			// we do care about validation code being actually different, since there is a check
			// that prevents upgrading to the same code.
			let old_validation_code = old_validation_code();
			let validation_code = validation_code();

			let mut parachains = ParachainsCache::new();
			Pallet::<T>::initialize_para_now(
				&mut parachains,
				id,
				&ParaGenesisArgs {
					parachain: true,
					genesis_head: HeadData(vec![1, 2, 3, 4]),
					validation_code: old_validation_code,
				},
			);
			// don't care about performance here, but we do care about robustness. So dump the cache
			// asap.
			drop(parachains);

			Pallet::<T>::schedule_code_upgrade(
				id,
				validation_code,
				/* relay_parent_number */ 1u32.into(),
				&configuration::Pallet::<T>::config(),
			);
		} else {
			let r = Pallet::<T>::schedule_para_initialize(
				id,
				ParaGenesisArgs {
					parachain: true,
					genesis_head: HeadData(vec![1, 2, 3, 4]),
					validation_code: validation_code(),
				},
			);
			assert!(r.is_ok());
		}
	}
}

/// Generates a list of votes combined with signatures for the active validator set. The number of
/// votes is equal to the minimum number of votes required to reach the supermajority.
fn generate_statements<T>(
	vote_outcome: VoteOutcome,
) -> impl Iterator<Item = (PvfCheckStatement, ValidatorSignature)>
where
	T: Config + shared::Config,
{
	let validators = ParasShared::<T>::active_validator_keys();

	let required_votes = primitives::v2::supermajority_threshold(validators.len());
	(0..required_votes).map(move |validator_index| {
		let stmt = PvfCheckStatement {
			accept: vote_outcome == VoteOutcome::Accept,
			subject: validation_code().hash(),
			session_index: SESSION_INDEX,

			validator_index: ValidatorIndex(validator_index as u32),
		};
		let signature = validators[validator_index].sign(&stmt.signing_payload()).unwrap();

		(stmt, signature)
	})
}

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

//! The actual implementation of the validate block functionality.

use frame_support::traits::{ExecuteBlock, ExtrinsicCall, Get, IsSubType};
use sp_runtime::traits::{Block as BlockT, Extrinsic, HashFor, Header as HeaderT};

use sp_io::KillStorageResult;
use sp_std::prelude::*;

use selendra_parachain::primitives::{HeadData, ValidationParams, ValidationResult};

use codec::{Decode, Encode};

use sp_core::storage::{ChildInfo, StateVersion};
use sp_externalities::{set_and_run_with_externalities, Externalities};
use sp_trie::MemoryDB;

type TrieBackend<B> = sp_state_machine::TrieBackend<MemoryDB<HashFor<B>>, HashFor<B>>;

type Ext<'a, B> = sp_state_machine::Ext<'a, HashFor<B>, TrieBackend<B>>;

fn with_externalities<F: FnOnce(&mut dyn Externalities) -> R, R>(f: F) -> R {
	sp_externalities::with_externalities(f).expect("Environmental externalities not set.")
}

/// Validate a given parachain block on a validator.
#[doc(hidden)]
pub fn validate_block<
	B: BlockT,
	E: ExecuteBlock<B>,
	PSC: crate::Config,
	CI: crate::CheckInherents<B>,
>(
	params: ValidationParams,
) -> ValidationResult
where
	B::Extrinsic: ExtrinsicCall,
	<B::Extrinsic as Extrinsic>::Call: IsSubType<crate::Call<PSC>>,
{
	let block_data =
		forests_primitives_core::ParachainBlockData::<B>::decode(&mut &params.block_data.0[..])
			.expect("Invalid parachain block data");

	let parent_head =
		B::Header::decode(&mut &params.parent_head.0[..]).expect("Invalid parent head");

	let (header, extrinsics, storage_proof) = block_data.deconstruct();

	let head_data = HeadData(header.encode());

	let block = B::new(header, extrinsics);
	assert!(parent_head.hash() == *block.header().parent_hash(), "Invalid parent hash",);

	// Create the db
	let db = match storage_proof.to_memory_db(Some(parent_head.state_root())) {
		Ok((db, _)) => db,
		Err(_) => panic!("Compact proof decoding failure."),
	};

	sp_std::mem::drop(storage_proof);

	let backend = sp_state_machine::TrieBackend::new(db, *parent_head.state_root());

	let _guard = (
		// Replace storage calls with our own implementations
		sp_io::storage::host_read.replace_implementation(host_storage_read),
		sp_io::storage::host_set.replace_implementation(host_storage_set),
		sp_io::storage::host_get.replace_implementation(host_storage_get),
		sp_io::storage::host_exists.replace_implementation(host_storage_exists),
		sp_io::storage::host_clear.replace_implementation(host_storage_clear),
		sp_io::storage::host_root.replace_implementation(host_storage_root),
		sp_io::storage::host_clear_prefix.replace_implementation(host_storage_clear_prefix),
		sp_io::storage::host_append.replace_implementation(host_storage_append),
		sp_io::storage::host_next_key.replace_implementation(host_storage_next_key),
		sp_io::storage::host_start_transaction
			.replace_implementation(host_storage_start_transaction),
		sp_io::storage::host_rollback_transaction
			.replace_implementation(host_storage_rollback_transaction),
		sp_io::storage::host_commit_transaction
			.replace_implementation(host_storage_commit_transaction),
		sp_io::default_child_storage::host_get
			.replace_implementation(host_default_child_storage_get),
		sp_io::default_child_storage::host_read
			.replace_implementation(host_default_child_storage_read),
		sp_io::default_child_storage::host_set
			.replace_implementation(host_default_child_storage_set),
		sp_io::default_child_storage::host_clear
			.replace_implementation(host_default_child_storage_clear),
		sp_io::default_child_storage::host_storage_kill
			.replace_implementation(host_default_child_storage_storage_kill),
		sp_io::default_child_storage::host_exists
			.replace_implementation(host_default_child_storage_exists),
		sp_io::default_child_storage::host_clear_prefix
			.replace_implementation(host_default_child_storage_clear_prefix),
		sp_io::default_child_storage::host_root
			.replace_implementation(host_default_child_storage_root),
		sp_io::default_child_storage::host_next_key
			.replace_implementation(host_default_child_storage_next_key),
		sp_io::offchain_index::host_set.replace_implementation(host_offchain_index_set),
		sp_io::offchain_index::host_clear.replace_implementation(host_offchain_index_clear),
	);

	let inherent_data = block
		.extrinsics()
		.iter()
		// Inherents are at the front of the block and are unsigned.
		//
		// If `is_signed` is returning `None`, we keep it safe and assume that it is "signed".
		// We are searching for unsigned transactions anyway.
		.take_while(|e| !e.is_signed().unwrap_or(true))
		.filter_map(|e| e.call().is_sub_type())
		.find_map(|c| match c {
			crate::Call::set_validation_data { data: validation_data } =>
				Some(validation_data.clone()),
			_ => None,
		})
		.expect("Could not find `set_validation_data` inherent");

	run_with_externalities::<B, _, _>(&backend, || {
		let relay_chain_proof = crate::RelayChainStateProof::new(
			PSC::SelfParaId::get(),
			inherent_data.validation_data.relay_parent_storage_root,
			inherent_data.relay_chain_state.clone(),
		)
		.expect("Invalid relay chain state proof");

		let res = CI::check_inherents(&block, &relay_chain_proof);

		if !res.ok() {
			if log::log_enabled!(log::Level::Error) {
				res.into_errors().for_each(|e| {
					log::error!("Checking inherent with identifier `{:?}` failed", e.0)
				});
			}

			panic!("Checking inherents failed");
		}
	});

	run_with_externalities::<B, _, _>(&backend, || {
		super::set_and_run_with_validation_params(params, || {
			E::execute_block(block);

			let new_validation_code = crate::NewValidationCode::<PSC>::get();
			let upward_messages = crate::UpwardMessages::<PSC>::get();
			let processed_downward_messages = crate::ProcessedDownwardMessages::<PSC>::get();
			let horizontal_messages = crate::HrmpOutboundMessages::<PSC>::get();
			let hrmp_watermark = crate::HrmpWatermark::<PSC>::get();

			let head_data =
				if let Some(custom_head_data) = crate::CustomValidationHeadData::<PSC>::get() {
					HeadData(custom_head_data)
				} else {
					head_data
				};

			ValidationResult {
				head_data,
				new_validation_code: new_validation_code.map(Into::into),
				upward_messages,
				processed_downward_messages,
				horizontal_messages,
				hrmp_watermark,
			}
		})
	})
}

/// Run the given closure with the externalities set.
fn run_with_externalities<B: BlockT, R, F: FnOnce() -> R>(
	backend: &TrieBackend<B>,
	execute: F,
) -> R {
	let mut overlay = sp_state_machine::OverlayedChanges::default();
	let mut cache = Default::default();
	let mut ext = Ext::<B>::new(&mut overlay, &mut cache, backend);

	set_and_run_with_externalities(&mut ext, || execute())
}

fn host_storage_read(key: &[u8], value_out: &mut [u8], value_offset: u32) -> Option<u32> {
	match with_externalities(|ext| ext.storage(key)) {
		Some(value) => {
			let value_offset = value_offset as usize;
			let data = &value[value_offset.min(value.len())..];
			let written = sp_std::cmp::min(data.len(), value_out.len());
			value_out[..written].copy_from_slice(&data[..written]);
			Some(value.len() as u32)
		},
		None => None,
	}
}

fn host_storage_set(key: &[u8], value: &[u8]) {
	with_externalities(|ext| ext.place_storage(key.to_vec(), Some(value.to_vec())))
}

fn host_storage_get(key: &[u8]) -> Option<bytes::Bytes> {
	with_externalities(|ext| ext.storage(key).map(|value| value.into()))
}

fn host_storage_exists(key: &[u8]) -> bool {
	with_externalities(|ext| ext.exists_storage(key))
}

fn host_storage_clear(key: &[u8]) {
	with_externalities(|ext| ext.place_storage(key.to_vec(), None))
}

fn host_storage_root(version: StateVersion) -> Vec<u8> {
	with_externalities(|ext| ext.storage_root(version))
}

fn host_storage_clear_prefix(prefix: &[u8], limit: Option<u32>) -> KillStorageResult {
	with_externalities(|ext| ext.clear_prefix(prefix, limit, None).into())
}

fn host_storage_append(key: &[u8], value: Vec<u8>) {
	with_externalities(|ext| ext.storage_append(key.to_vec(), value))
}

fn host_storage_next_key(key: &[u8]) -> Option<Vec<u8>> {
	with_externalities(|ext| ext.next_storage_key(key))
}

fn host_storage_start_transaction() {
	with_externalities(|ext| ext.storage_start_transaction())
}

fn host_storage_rollback_transaction() {
	with_externalities(|ext| ext.storage_rollback_transaction().ok())
		.expect("No open transaction that can be rolled back.");
}

fn host_storage_commit_transaction() {
	with_externalities(|ext| ext.storage_commit_transaction().ok())
		.expect("No open transaction that can be committed.");
}

fn host_default_child_storage_get(storage_key: &[u8], key: &[u8]) -> Option<Vec<u8>> {
	let child_info = ChildInfo::new_default(storage_key);
	with_externalities(|ext| ext.child_storage(&child_info, key))
}

fn host_default_child_storage_read(
	storage_key: &[u8],
	key: &[u8],
	value_out: &mut [u8],
	value_offset: u32,
) -> Option<u32> {
	let child_info = ChildInfo::new_default(storage_key);
	match with_externalities(|ext| ext.child_storage(&child_info, key)) {
		Some(value) => {
			let value_offset = value_offset as usize;
			let data = &value[value_offset.min(value.len())..];
			let written = sp_std::cmp::min(data.len(), value_out.len());
			value_out[..written].copy_from_slice(&data[..written]);
			Some(value.len() as u32)
		},
		None => None,
	}
}

fn host_default_child_storage_set(storage_key: &[u8], key: &[u8], value: &[u8]) {
	let child_info = ChildInfo::new_default(storage_key);
	with_externalities(|ext| {
		ext.place_child_storage(&child_info, key.to_vec(), Some(value.to_vec()))
	})
}

fn host_default_child_storage_clear(storage_key: &[u8], key: &[u8]) {
	let child_info = ChildInfo::new_default(storage_key);
	with_externalities(|ext| ext.place_child_storage(&child_info, key.to_vec(), None))
}

fn host_default_child_storage_storage_kill(
	storage_key: &[u8],
	limit: Option<u32>,
) -> KillStorageResult {
	let child_info = ChildInfo::new_default(storage_key);
	with_externalities(|ext| ext.kill_child_storage(&child_info, limit, None).into())
}

fn host_default_child_storage_exists(storage_key: &[u8], key: &[u8]) -> bool {
	let child_info = ChildInfo::new_default(storage_key);
	with_externalities(|ext| ext.exists_child_storage(&child_info, key))
}

fn host_default_child_storage_clear_prefix(
	storage_key: &[u8],
	prefix: &[u8],
	limit: Option<u32>,
) -> KillStorageResult {
	let child_info = ChildInfo::new_default(storage_key);
	with_externalities(|ext| ext.clear_child_prefix(&child_info, prefix, limit, None).into())
}

fn host_default_child_storage_root(storage_key: &[u8], version: StateVersion) -> Vec<u8> {
	let child_info = ChildInfo::new_default(storage_key);
	with_externalities(|ext| ext.child_storage_root(&child_info, version))
}

fn host_default_child_storage_next_key(storage_key: &[u8], key: &[u8]) -> Option<Vec<u8>> {
	let child_info = ChildInfo::new_default(storage_key);
	with_externalities(|ext| ext.next_child_storage_key(&child_info, key))
}

fn host_offchain_index_set(_key: &[u8], _value: &[u8]) {}

fn host_offchain_index_clear(_key: &[u8]) {}

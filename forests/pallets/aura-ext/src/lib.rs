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

//! Forests extension pallet for AuRa
//!
//! This pallets extends the Substrate AuRa pallet to make it compatible with parachains. It
//! provides the [`Pallet`], the [`Config`] and the [`GenesisConfig`].
//!
//! It is also required that the parachain runtime uses the provided [`BlockExecutor`] to properly
//! check the constructed block on the relay chain.
//!
//! ```
//!# struct Runtime;
//!# struct Executive;
//!# struct CheckInherents;
//! forests_pallet_parachain_system::register_validate_block! {
//!     Runtime = Runtime,
//!     BlockExecutor = forests_pallet_aura_ext::BlockExecutor::<Runtime, Executive>,
//!     CheckInherents = CheckInherents,
//! }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::traits::{ExecuteBlock, FindAuthor};
use sp_application_crypto::RuntimeAppPublic;
use sp_consensus_aura::digests::CompatibleDigestItem;
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};

type Aura<T> = pallet_aura::Pallet<T>;

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	/// The configuration trait.
	#[pallet::config]
	pub trait Config: pallet_aura::Config + frame_system::Config {}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_finalize(_: BlockNumberFor<T>) {
			// Update to the latest AuRa authorities.
			Authorities::<T>::put(Aura::<T>::authorities());
		}

		fn on_initialize(_: BlockNumberFor<T>) -> Weight {
			// Fetch the authorities once to get them into the storage proof of the PoV.
			Authorities::<T>::get();

			T::DbWeight::get().reads_writes(2, 1)
		}
	}

	/// Serves as cache for the authorities.
	///
	/// The authorities in AuRa are overwritten in `on_initialize` when we switch to a new session,
	/// but we require the old authorities to verify the seal when validating a PoV. This will always
	/// be updated to the latest AuRa authorities in `on_finalize`.
	#[pallet::storage]
	pub(crate) type Authorities<T: Config> = StorageValue<
		_,
		BoundedVec<T::AuthorityId, <T as pallet_aura::Config>::MaxAuthorities>,
		ValueQuery,
	>;

	#[pallet::genesis_config]
	#[derive(Default)]
	pub struct GenesisConfig;

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig {
		fn build(&self) {
			let authorities = Aura::<T>::authorities();

			assert!(
				!authorities.is_empty(),
				"AuRa authorities empty, maybe wrong order in `construct_runtime!`?",
			);

			Authorities::<T>::put(authorities);
		}
	}
}

/// The block executor used when validating a PoV at the relay chain.
///
/// When executing the block it will verify the block seal to ensure that the correct author created
/// the block.
pub struct BlockExecutor<T, I>(sp_std::marker::PhantomData<(T, I)>);

impl<Block, T, I> ExecuteBlock<Block> for BlockExecutor<T, I>
where
	Block: BlockT,
	T: Config,
	I: ExecuteBlock<Block>,
{
	fn execute_block(block: Block) {
		let (mut header, extrinsics) = block.deconstruct();
		// We need to fetch the authorities before we execute the block, to get the authorities
		// before any potential update.
		let authorities = Authorities::<T>::get();

		let mut seal = None;
		header.digest_mut().logs.retain(|s| {
			let s =
				CompatibleDigestItem::<<T::AuthorityId as RuntimeAppPublic>::Signature>::as_aura_seal(s);
			match (s, seal.is_some()) {
				(Some(_), true) => panic!("Found multiple AuRa seal digests"),
				(None, _) => true,
				(Some(s), false) => {
					seal = Some(s);
					false
				},
			}
		});

		let seal = seal.expect("Could not find an AuRa seal digest!");

		let author = Aura::<T>::find_author(
			header.digest().logs().iter().filter_map(|d| d.as_pre_runtime()),
		)
		.expect("Could not find AuRa author index!");

		let pre_hash = header.hash();

		if !authorities
			.get(author as usize)
			.unwrap_or_else(|| {
				panic!("Invalid AuRa author index {} for authorities: {:?}", author, authorities)
			})
			.verify(&pre_hash, &seal)
		{
			panic!("Invalid AuRa seal");
		}

		I::execute_block(Block::new(header, extrinsics));
	}
}

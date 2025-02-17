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

use crate::{Config, Pallet};
use frame_support::{
	traits::{Get, StorageVersion},
	weights::Weight,
};

/// The current storage version.
pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

/// Call this during the next runtime upgrade for this module.
pub fn on_runtime_upgrade<T: Config>() -> Weight {
	let mut weight: Weight = 0;

	if StorageVersion::get::<Pallet<T>>() == 0 {
		weight = weight
			.saturating_add(v1::migrate::<T>())
			.saturating_add(T::DbWeight::get().writes(1));
		StorageVersion::new(1).put::<Pallet<T>>();
	}

	weight
}

/// V1: `LastUpgrade` block number is removed from the storage since the upgrade
/// mechanism now uses signals instead of block offsets.
mod v1 {
	use crate::{Config, Pallet};
	#[allow(deprecated)]
	use frame_support::{migration::remove_storage_prefix, pallet_prelude::*};

	pub fn migrate<T: Config>() -> Weight {
		#[allow(deprecated)]
		remove_storage_prefix(<Pallet<T>>::name().as_bytes(), b"LastUpgrade", b"");
		T::DbWeight::get().writes(1)
	}
}

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

//! Various implementations of `FilterAssetLocation`.

use frame_support::traits::Get;
use sp_std::marker::PhantomData;
use xcm::latest::{AssetId::Concrete, MultiAsset, MultiAssetFilter, MultiLocation};
use xcm_executor::traits::FilterAssetLocation;

/// Accepts an asset iff it is a native asset.
pub struct NativeAsset;
impl FilterAssetLocation for NativeAsset {
	fn filter_asset_location(asset: &MultiAsset, origin: &MultiLocation) -> bool {
		log::trace!(target: "xcm::filter_asset_location", "NativeAsset asset: {:?}, origin: {:?}", asset, origin);
		matches!(asset.id, Concrete(ref id) if id == origin)
	}
}

/// Accepts an asset if it is contained in the given `T`'s `Get` implementation.
pub struct Case<T>(PhantomData<T>);
impl<T: Get<(MultiAssetFilter, MultiLocation)>> FilterAssetLocation for Case<T> {
	fn filter_asset_location(asset: &MultiAsset, origin: &MultiLocation) -> bool {
		log::trace!(target: "xcm::filter_asset_location", "Case asset: {:?}, origin: {:?}", asset, origin);
		let (a, o) = T::get();
		a.contains(asset) && &o == origin
	}
}

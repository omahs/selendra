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
use crate::Assets;
use core::marker::PhantomData;
use frame_support::{traits::Contains, weights::Weight};
use xcm::latest::{MultiAssets, MultiLocation};

/// Define a handler for when some non-empty `Assets` value should be dropped.
pub trait DropAssets {
	/// Handler for receiving dropped assets. Returns the weight consumed by this operation.
	fn drop_assets(origin: &MultiLocation, assets: Assets) -> Weight;
}
impl DropAssets for () {
	fn drop_assets(_origin: &MultiLocation, _assets: Assets) -> Weight {
		0
	}
}

/// Morph a given `DropAssets` implementation into one which can filter based on assets. This can
/// be used to ensure that `Assets` values which hold no value are ignored.
pub struct FilterAssets<D, A>(PhantomData<(D, A)>);

impl<D: DropAssets, A: Contains<Assets>> DropAssets for FilterAssets<D, A> {
	fn drop_assets(origin: &MultiLocation, assets: Assets) -> Weight {
		if A::contains(&assets) {
			D::drop_assets(origin, assets)
		} else {
			0
		}
	}
}

/// Morph a given `DropAssets` implementation into one which can filter based on origin. This can
/// be used to ban origins which don't have proper protections/policies against misuse of the
/// asset trap facility don't get to use it.
pub struct FilterOrigin<D, O>(PhantomData<(D, O)>);

impl<D: DropAssets, O: Contains<MultiLocation>> DropAssets for FilterOrigin<D, O> {
	fn drop_assets(origin: &MultiLocation, assets: Assets) -> Weight {
		if O::contains(origin) {
			D::drop_assets(origin, assets)
		} else {
			0
		}
	}
}

/// Define any handlers for the `AssetClaim` instruction.
pub trait ClaimAssets {
	/// Claim any assets available to `origin` and return them in a single `Assets` value, together
	/// with the weight used by this operation.
	fn claim_assets(origin: &MultiLocation, ticket: &MultiLocation, what: &MultiAssets) -> bool;
}

#[impl_trait_for_tuples::impl_for_tuples(30)]
impl ClaimAssets for Tuple {
	fn claim_assets(origin: &MultiLocation, ticket: &MultiLocation, what: &MultiAssets) -> bool {
		for_tuples!( #(
			if Tuple::claim_assets(origin, ticket, what) {
				return true;
			}
		)* );
		false
	}
}

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
use sp_std::result;
use xcm::latest::{Error as XcmError, MultiAsset};

/// Errors associated with [`MatchesFungibles`] operation.
pub enum Error {
	/// Asset not found.
	AssetNotFound,
	/// `MultiLocation` to `AccountId` conversion failed.
	AccountIdConversionFailed,
	/// `u128` amount to currency `Balance` conversion failed.
	AmountToBalanceConversionFailed,
	/// `MultiLocation` to `AssetId` conversion failed.
	AssetIdConversionFailed,
}

impl From<Error> for XcmError {
	fn from(e: Error) -> Self {
		use XcmError::FailedToTransactAsset;
		match e {
			Error::AssetNotFound => XcmError::AssetNotFound,
			Error::AccountIdConversionFailed => FailedToTransactAsset("AccountIdConversionFailed"),
			Error::AmountToBalanceConversionFailed =>
				FailedToTransactAsset("AmountToBalanceConversionFailed"),
			Error::AssetIdConversionFailed => FailedToTransactAsset("AssetIdConversionFailed"),
		}
	}
}

pub trait MatchesFungibles<AssetId, Balance> {
	fn matches_fungibles(a: &MultiAsset) -> result::Result<(AssetId, Balance), Error>;
}

#[impl_trait_for_tuples::impl_for_tuples(30)]
impl<AssetId: Clone, Balance: Clone> MatchesFungibles<AssetId, Balance> for Tuple {
	fn matches_fungibles(a: &MultiAsset) -> result::Result<(AssetId, Balance), Error> {
		for_tuples!( #(
			match Tuple::matches_fungibles(a) { o @ Ok(_) => return o, _ => () }
		)* );
		log::trace!(target: "xcm::matches_fungibles", "did not match fungibles asset: {:?}", &a);
		Err(Error::AssetNotFound)
	}
}

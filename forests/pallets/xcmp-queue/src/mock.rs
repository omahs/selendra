// Copyright (C) 2021-2022 Selendra.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use crate as xcmp_queue;
use core::marker::PhantomData;
use forests_pallet_parachain_system::AnyRelayNumber;
use forests_primitives_core::{IsSystem, ParaId};
use frame_support::{parameter_types, traits::OriginTrait};
use frame_system::EnsureRoot;
use sp_core::H256;
use sp_runtime::{
	testing::Header,
	traits::{BlakeTwo256, IdentityLookup},
};
use xcm::prelude::*;
use xcm_builder::{
	CurrencyAdapter, FixedWeightBounds, IsConcrete, LocationInverter, NativeAsset, ParentIsPreset,
};
use xcm_executor::traits::ConvertOrigin;

type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
type Block = frame_system::mocking::MockBlock<Test>;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test where
		Block = Block,
		NodeBlock = Block,
		UncheckedExtrinsic = UncheckedExtrinsic,
	{
		System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
		Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
		ParachainSystem: forests_pallet_parachain_system::{
			Pallet, Call, Config, Storage, Inherent, Event<T>, ValidateUnsigned,
		},
		XcmpQueue: xcmp_queue::{Pallet, Call, Storage, Event<T>},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

type AccountId = u64;

impl frame_system::Config for Test {
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type Origin = Origin;
	type Call = Call;
	type Index = u64;
	type BlockNumber = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Header = Header;
	type Event = Event;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<u64>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = forests_pallet_parachain_system::ParachainSetCode<Test>;
	type MaxConsumers = frame_support::traits::ConstU32<16>;
}

parameter_types! {
	pub const ExistentialDeposit: u64 = 5;
	pub const MaxReserves: u32 = 50;
}

impl pallet_balances::Config for Test {
	type Balance = u64;
	type Event = Event;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = ();
	type MaxLocks = ();
	type MaxReserves = MaxReserves;
	type ReserveIdentifier = [u8; 8];
}

impl forests_pallet_parachain_system::Config for Test {
	type Event = Event;
	type OnSystemEvent = ();
	type SelfParaId = ();
	type OutboundXcmpMessageSource = XcmpQueue;
	type DmpMessageHandler = ();
	type ReservedDmpWeight = ();
	type XcmpMessageHandler = XcmpQueue;
	type ReservedXcmpWeight = ();
	type CheckAssociatedRelayNumber = AnyRelayNumber;
}

parameter_types! {
	pub const RelayChain: MultiLocation = MultiLocation::parent();
	pub Ancestry: MultiLocation = X1(Parachain(1u32.into())).into();
	pub UnitWeightCost: Weight = 1_000_000;
	pub const MaxInstructions: u32 = 100;
}

/// Means for transacting assets on this chain.
pub type LocalAssetTransactor = CurrencyAdapter<
	// Use this currency:
	Balances,
	// Use this currency when it is a fungible asset matching the given location or name:
	IsConcrete<RelayChain>,
	// Do a simple punn to convert an AccountId32 MultiLocation into a native chain account ID:
	LocationToAccountId,
	// Our chain's account ID type (we can't get away without mentioning it explicitly):
	AccountId,
	// We don't track any teleports.
	(),
>;

pub type LocationToAccountId = (ParentIsPreset<AccountId>,);

pub struct XcmConfig;
impl xcm_executor::Config for XcmConfig {
	type Call = Call;
	type XcmSender = XcmRouter;
	// How to withdraw and deposit an asset.
	type AssetTransactor = LocalAssetTransactor;
	type OriginConverter = ();
	type IsReserve = NativeAsset;
	type IsTeleporter = NativeAsset;
	type LocationInverter = LocationInverter<Ancestry>;
	type Barrier = ();
	type Weigher = FixedWeightBounds<UnitWeightCost, Call, MaxInstructions>;
	type Trader = ();
	type ResponseHandler = ();
	type AssetTrap = ();
	type AssetClaims = ();
	type SubscriptionService = ();
}

pub type XcmRouter = (
	// XCMP to communicate with the sibling chains.
	XcmpQueue,
);

pub struct SystemParachainAsSuperuser<Origin>(PhantomData<Origin>);
impl<Origin: OriginTrait> ConvertOrigin<Origin> for SystemParachainAsSuperuser<Origin> {
	fn convert_origin(
		origin: impl Into<MultiLocation>,
		kind: OriginKind,
	) -> Result<Origin, MultiLocation> {
		let origin = origin.into();
		if kind == OriginKind::Superuser &&
			matches!(
				origin,
				MultiLocation {
					parents: 1,
					interior: X1(Parachain(id)),
				} if ParaId::from(id).is_system(),
			) {
			Ok(Origin::root())
		} else {
			Err(origin)
		}
	}
}

impl Config for Test {
	type Event = Event;
	type XcmExecutor = xcm_executor::XcmExecutor<XcmConfig>;
	type ChannelInfo = ParachainSystem;
	type VersionWrapper = ();
	type ExecuteOverweightOrigin = EnsureRoot<AccountId>;
	type ControllerOrigin = EnsureRoot<AccountId>;
	type ControllerOriginConverter = SystemParachainAsSuperuser<Origin>;
	type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();
	t.into()
}

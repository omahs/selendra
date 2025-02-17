// This file is part of Selendra.

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

//! A list of the different weight modules for our runtime.
#![allow(clippy::unnecessary_cast)]
pub mod frame_system;

pub mod pallet_bags_list;
pub mod pallet_balances;
pub mod pallet_bounties;
pub mod pallet_collective;
pub mod pallet_democracy;
pub mod pallet_election_provider_multi_phase;
pub mod pallet_elections_phragmen;
pub mod pallet_identity;
pub mod pallet_im_online;
pub mod pallet_indices;
pub mod pallet_membership;
pub mod pallet_multisig;
pub mod pallet_preimage;
pub mod pallet_proxy;
pub mod pallet_scheduler;
pub mod pallet_session;
pub mod pallet_staking;
pub mod pallet_timestamp;
pub mod pallet_tips;
pub mod pallet_treasury;
pub mod pallet_utility;
pub mod pallet_vesting;

pub mod runtime_common_paras_registrar;
pub mod runtime_common_slots;
pub mod runtime_parachains_configuration;
pub mod runtime_parachains_disputes;
pub mod runtime_parachains_hrmp;
pub mod runtime_parachains_initializer;
pub mod runtime_parachains_paras;
pub mod runtime_parachains_paras_inherent;

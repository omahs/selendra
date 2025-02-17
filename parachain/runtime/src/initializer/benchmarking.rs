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

use super::*;
use frame_benchmarking::benchmarks;
use frame_system::RawOrigin;
use primitives::v2::ConsensusLog;
use sp_runtime::DigestItem;

// Random large number for the digest
const DIGEST_MAX_LEN: u32 = 65536;

benchmarks! {
	force_approve {
		let d in 0 .. DIGEST_MAX_LEN;
		for _ in 0 .. d {
			<frame_system::Pallet<T>>::deposit_log(ConsensusLog::ForceApprove(d).into());
		}
	}: _(RawOrigin::Root, d + 1)
	verify {
		assert_eq!(
			<frame_system::Pallet<T>>::digest().logs.last().unwrap(),
			&DigestItem::from(ConsensusLog::ForceApprove(d + 1)),
		);
	}

	impl_benchmark_test_suite!(
		Pallet,
		crate::mock::new_test_ext(Default::default()),
		crate::mock::Test
	);
}

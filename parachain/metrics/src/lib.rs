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

//! Runtime metric interface similar to native Prometheus metrics.
//!
//! This is intended to be used only for testing and debugging and **must never
//! be used in production**. It requires the Substrate wasm tracing support
//! and command line configuration: `--tracing-targets wasm_tracing=trace`.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "runtime-metrics")]
mod with_runtime_metrics;
#[cfg(feature = "runtime-metrics")]
pub use crate::with_runtime_metrics::*;

#[cfg(not(feature = "runtime-metrics"))]
mod without_runtime_metrics;
#[cfg(not(feature = "runtime-metrics"))]
pub use crate::without_runtime_metrics::*;

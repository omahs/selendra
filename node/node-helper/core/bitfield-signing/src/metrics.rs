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

use selendra_node_subsystem_util::metrics::{self, prometheus};

#[derive(Clone)]
pub(crate) struct MetricsInner {
	pub(crate) bitfields_signed_total: prometheus::Counter<prometheus::U64>,
	pub(crate) run: prometheus::Histogram,
}

/// Bitfield signing metrics.
#[derive(Default, Clone)]
pub struct Metrics(pub(crate) Option<MetricsInner>);

impl Metrics {
	pub fn on_bitfield_signed(&self) {
		if let Some(metrics) = &self.0 {
			metrics.bitfields_signed_total.inc();
		}
	}

	/// Provide a timer for `prune_povs` which observes on drop.
	pub fn time_run(&self) -> Option<metrics::prometheus::prometheus::HistogramTimer> {
		self.0.as_ref().map(|metrics| metrics.run.start_timer())
	}
}

impl metrics::Metrics for Metrics {
	fn try_register(registry: &prometheus::Registry) -> Result<Self, prometheus::PrometheusError> {
		let metrics = MetricsInner {
			bitfields_signed_total: prometheus::register(
				prometheus::Counter::new(
					"selendra_parachain_bitfields_signed_total",
					"Number of bitfields signed.",
				)?,
				registry,
			)?,
			run: prometheus::register(
				prometheus::Histogram::with_opts(prometheus::HistogramOpts::new(
					"selendra_parachain_bitfield_signing_run",
					"Time spent within `bitfield_signing::run`",
				))?,
				registry,
			)?,
		};
		Ok(Metrics(Some(metrics)))
	}
}

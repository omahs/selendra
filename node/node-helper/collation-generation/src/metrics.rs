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
	pub(crate) collations_generated_total: prometheus::Counter<prometheus::U64>,
	pub(crate) new_activations_overall: prometheus::Histogram,
	pub(crate) new_activations_per_relay_parent: prometheus::Histogram,
	pub(crate) new_activations_per_availability_core: prometheus::Histogram,
}

/// `CollationGenerationSubsystem` metrics.
#[derive(Default, Clone)]
pub struct Metrics(pub(crate) Option<MetricsInner>);

impl Metrics {
	pub fn on_collation_generated(&self) {
		if let Some(metrics) = &self.0 {
			metrics.collations_generated_total.inc();
		}
	}

	/// Provide a timer for new activations which updates on drop.
	pub fn time_new_activations(&self) -> Option<metrics::prometheus::prometheus::HistogramTimer> {
		self.0.as_ref().map(|metrics| metrics.new_activations_overall.start_timer())
	}

	/// Provide a timer per relay parents which updates on drop.
	pub fn time_new_activations_relay_parent(
		&self,
	) -> Option<metrics::prometheus::prometheus::HistogramTimer> {
		self.0
			.as_ref()
			.map(|metrics| metrics.new_activations_per_relay_parent.start_timer())
	}

	/// Provide a timer per availability core which updates on drop.
	pub fn time_new_activations_availability_core(
		&self,
	) -> Option<metrics::prometheus::prometheus::HistogramTimer> {
		self.0
			.as_ref()
			.map(|metrics| metrics.new_activations_per_availability_core.start_timer())
	}
}

impl metrics::Metrics for Metrics {
	fn try_register(registry: &prometheus::Registry) -> Result<Self, prometheus::PrometheusError> {
		let metrics = MetricsInner {
			collations_generated_total: prometheus::register(
				prometheus::Counter::new(
					"selendra_parachain_collations_generated_total",
					"Number of collations generated."
				)?,
				registry,
			)?,
			new_activations_overall: prometheus::register(
				prometheus::Histogram::with_opts(
					prometheus::HistogramOpts::new(
						"selendra_parachain_collation_generation_new_activations",
						"Time spent within fn handle_new_activations",
					)
				)?,
				registry,
			)?,
			new_activations_per_relay_parent: prometheus::register(
				prometheus::Histogram::with_opts(
					prometheus::HistogramOpts::new(
						"selendra_parachain_collation_generation_per_relay_parent",
						"Time spent handling a particular relay parent within fn handle_new_activations"
					)
				)?,
				registry,
			)?,
			new_activations_per_availability_core: prometheus::register(
				prometheus::Histogram::with_opts(
					prometheus::HistogramOpts::new(
						"selendra_parachain_collation_generation_per_availability_core",
						"Time spent handling a particular availability core for a relay parent in fn handle_new_activations",
					)
				)?,
				registry,
			)?,
		};
		Ok(Metrics(Some(metrics)))
	}
}

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
struct MetricsInner {
	/// Number of opened disputes.
	open: prometheus::Counter<prometheus::U64>,
	/// Votes of all disputes.
	votes: prometheus::CounterVec<prometheus::U64>,
	/// Number of approval votes explicitly fetched from approval voting.
	approval_votes: prometheus::Counter<prometheus::U64>,
	/// Conclusion across all disputes.
	concluded: prometheus::CounterVec<prometheus::U64>,
	/// Number of participations that have been queued.
	queued_participations: prometheus::CounterVec<prometheus::U64>,
	/// How long vote cleanup batches take.
	vote_cleanup_time: prometheus::Histogram,
}

/// Candidate validation metrics.
#[derive(Default, Clone)]
pub struct Metrics(Option<MetricsInner>);

impl Metrics {
	pub(crate) fn on_open(&self) {
		if let Some(metrics) = &self.0 {
			metrics.open.inc();
		}
	}

	pub(crate) fn on_valid_votes(&self, vote_count: u32) {
		if let Some(metrics) = &self.0 {
			metrics.votes.with_label_values(&["valid"]).inc_by(vote_count as _);
		}
	}

	pub(crate) fn on_invalid_votes(&self, vote_count: u32) {
		if let Some(metrics) = &self.0 {
			metrics.votes.with_label_values(&["invalid"]).inc_by(vote_count as _);
		}
	}

	pub(crate) fn on_approval_votes(&self, vote_count: u32) {
		if let Some(metrics) = &self.0 {
			metrics.approval_votes.inc_by(vote_count as _);
		}
	}

	pub(crate) fn on_concluded_valid(&self) {
		if let Some(metrics) = &self.0 {
			metrics.concluded.with_label_values(&["valid"]).inc();
		}
	}

	pub(crate) fn on_concluded_invalid(&self) {
		if let Some(metrics) = &self.0 {
			metrics.concluded.with_label_values(&["invalid"]).inc();
		}
	}

	pub(crate) fn on_queued_priority_participation(&self) {
		if let Some(metrics) = &self.0 {
			metrics.queued_participations.with_label_values(&["priority"]).inc();
		}
	}

	pub(crate) fn on_queued_best_effort_participation(&self) {
		if let Some(metrics) = &self.0 {
			metrics.queued_participations.with_label_values(&["best-effort"]).inc();
		}
	}

	pub(crate) fn time_vote_cleanup(&self) -> Option<prometheus::prometheus::HistogramTimer> {
		self.0.as_ref().map(|metrics| metrics.vote_cleanup_time.start_timer())
	}
}

impl metrics::Metrics for Metrics {
	fn try_register(registry: &prometheus::Registry) -> Result<Self, prometheus::PrometheusError> {
		let metrics = MetricsInner {
			open: prometheus::register(
				prometheus::Counter::with_opts(prometheus::Opts::new(
					"selendra_parachain_candidate_disputes_total",
					"Total number of raised disputes.",
				))?,
				registry,
			)?,
			concluded: prometheus::register(
				prometheus::CounterVec::new(
					prometheus::Opts::new(
						"selendra_parachain_candidate_dispute_concluded",
						"Concluded dispute votes, sorted by candidate is `valid` and `invalid`.",
					),
					&["validity"],
				)?,
				registry,
			)?,
			votes: prometheus::register(
				prometheus::CounterVec::new(
					prometheus::Opts::new(
						"selendra_parachain_candidate_dispute_votes",
						"Accumulated dispute votes, sorted by candidate is `valid` and `invalid`.",
					),
					&["validity"],
				)?,
				registry,
			)?,
			approval_votes: prometheus::register(
				prometheus::Counter::with_opts(prometheus::Opts::new(
					"selendra_parachain_dispute_candidate_approval_votes_fetched_total",
					"Number of approval votes fetched from approval voting.",
				))?,
				registry,
			)?,
			queued_participations: prometheus::register(
				prometheus::CounterVec::new(
					prometheus::Opts::new(
						"selendra_parachain_dispute_participations",
						"Total number of queued participations, grouped by priority and best-effort. (Not every queueing will necessarily lead to an actual participation because of duplicates.)",
					),
					&["priority"],
				)?,
				registry,
			)?,
			vote_cleanup_time: prometheus::register(
				prometheus::Histogram::with_opts(
					prometheus::HistogramOpts::new(
						"selendra_parachain_dispute_coordinator_vote_cleanup",
						"Time spent cleaning up old votes per batch.",
					)
					.buckets([0.01, 0.1, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0].into()),
				)?,
				registry,
			)?,
		};
		Ok(Metrics(Some(metrics)))
	}
}

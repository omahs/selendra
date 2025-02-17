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

use std::{
	collections::{HashMap, HashSet},
	convert::TryFrom,
};

pub use sc_network::{PeerId, ReputationChange};

use selendra_node_network_protocol::{
	grid_topology::SessionGridTopology, ObservedRole, OurView, ProtocolVersion, View, WrongVariant,
};
use selendra_primitives::v2::{AuthorityDiscoveryId, SessionIndex, ValidatorIndex};

/// Information about a peer in the gossip topology for a session.
#[derive(Debug, Clone, PartialEq)]
pub struct TopologyPeerInfo {
	/// The validator's known peer IDs.
	pub peer_ids: Vec<PeerId>,
	/// The index of the validator in the discovery keys of the corresponding
	/// `SessionInfo`. This can extend _beyond_ the set of active parachain validators.
	pub validator_index: ValidatorIndex,
}

/// A struct indicating new gossip topology.
#[derive(Debug, Clone, PartialEq)]
pub struct NewGossipTopology {
	/// The session index this topology corresponds to.
	pub session: SessionIndex,
	/// Neighbors in the 'X' dimension of the grid.
	pub our_neighbors_x: HashMap<AuthorityDiscoveryId, TopologyPeerInfo>,
	/// Neighbors in the 'Y' dimension of the grid.
	pub our_neighbors_y: HashMap<AuthorityDiscoveryId, TopologyPeerInfo>,
}

/// Events from network.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkBridgeTxEvent<M> {
	/// A peer has connected.
	PeerConnected(PeerId, ObservedRole, ProtocolVersion, Option<HashSet<AuthorityDiscoveryId>>),

	/// A peer has disconnected.
	PeerDisconnected(PeerId),

	/// Our neighbors in the new gossip topology for the session.
	/// We're not necessarily connected to all of them.
	///
	/// This message is issued only on the validation peer set.
	///
	/// Note, that the distribution subsystems need to handle the last
	/// view update of the newly added gossip peers manually.
	NewGossipTopology(NewGossipTopology),

	/// Peer has sent a message.
	PeerMessage(PeerId, M),

	/// Peer's `View` has changed.
	PeerViewChange(PeerId, View),

	/// Our view has changed.
	OurViewChange(OurView),
}

impl<M> NetworkBridgeTxEvent<M> {
	/// Focus an overarching network-bridge event into some more specific variant.
	///
	/// This tries to transform M in `PeerMessage` to a message type specific to a subsystem.
	/// It is used to dispatch events coming from a peer set to the various subsystems that are
	/// handled within that peer set. More concretely a `ValidationProtocol` will be transformed
	/// for example into a `BitfieldDistributionMessage` in case of the `BitfieldDistribution`
	/// constructor.
	///
	/// Therefore a `NetworkBridgeTxEvent<ValidationProtocol>` will become for example a
	/// `NetworkBridgeTxEvent<BitfieldDistributionMessage>`, with the more specific message type
	/// `BitfieldDistributionMessage`.
	///
	/// This acts as a call to `clone`, except in the case where the event is a message event,
	/// in which case the clone can be expensive and it only clones if the message type can
	/// be focused.
	pub fn focus<'a, T>(&'a self) -> Result<NetworkBridgeTxEvent<T>, WrongVariant>
	where
		T: 'a + Clone,
		T: TryFrom<&'a M, Error = WrongVariant>,
	{
		Ok(match *self {
			NetworkBridgeTxEvent::PeerMessage(ref peer, ref msg) =>
				NetworkBridgeTxEvent::PeerMessage(peer.clone(), T::try_from(msg)?),
			NetworkBridgeTxEvent::PeerConnected(
				ref peer,
				ref role,
				ref version,
				ref authority_id,
			) => NetworkBridgeTxEvent::PeerConnected(
				peer.clone(),
				role.clone(),
				*version,
				authority_id.clone(),
			),
			NetworkBridgeTxEvent::PeerDisconnected(ref peer) =>
				NetworkBridgeTxEvent::PeerDisconnected(peer.clone()),
			NetworkBridgeTxEvent::NewGossipTopology(ref topology) =>
				NetworkBridgeTxEvent::NewGossipTopology(topology.clone()),
			NetworkBridgeTxEvent::PeerViewChange(ref peer, ref view) =>
				NetworkBridgeTxEvent::PeerViewChange(peer.clone(), view.clone()),
			NetworkBridgeTxEvent::OurViewChange(ref view) =>
				NetworkBridgeTxEvent::OurViewChange(view.clone()),
		})
	}
}

impl From<NewGossipTopology> for SessionGridTopology {
	fn from(topology: NewGossipTopology) -> Self {
		let peers_x =
			topology.our_neighbors_x.values().flat_map(|p| &p.peer_ids).cloned().collect();
		let peers_y =
			topology.our_neighbors_y.values().flat_map(|p| &p.peer_ids).cloned().collect();

		let validator_indices_x =
			topology.our_neighbors_x.values().map(|p| p.validator_index.clone()).collect();
		let validator_indices_y =
			topology.our_neighbors_y.values().map(|p| p.validator_index.clone()).collect();

		SessionGridTopology { peers_x, peers_y, validator_indices_x, validator_indices_y }
	}
}

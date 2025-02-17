// Copyright (C) 2021-2022 Selendra.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// Selendra is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Selendra is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Selendra.  If not, see <http://www.gnu.org/licenses/>.

//! Selendra service. Specialized wrapper over substrate service.

#![deny(unused_results)]

pub mod chain_spec;
mod grandpa_support;
mod parachains_db;
mod relay_chain_selection;

#[cfg(test)]
mod tests;

#[cfg(feature = "full-node")]
pub mod overseer;

#[cfg(feature = "full-node")]
pub use self::overseer::{OverseerGen, OverseerGenArgs, RealOverseerGen};

#[cfg(feature = "full-node")]
use {
	gum::info,
	sc_client_api::{BlockBackend, ExecutorProvider},
	sc_finality_grandpa::{self, FinalityProofProvider as GrandpaFinalityProofProvider},
	selendra_node_core_approval_voting::{
		self as approval_voting_subsystem, Config as ApprovalVotingConfig,
	},
	selendra_node_core_av_store::Config as AvailabilityConfig,
	selendra_node_core_av_store::Error as AvailabilityError,
	selendra_node_core_candidate_validation::Config as CandidateValidationConfig,
	selendra_node_core_chain_selection::{
		self as chain_selection_subsystem, Config as ChainSelectionConfig,
	},
	selendra_node_core_dispute_coordinator::Config as DisputeCoordinatorConfig,
	selendra_node_network_protocol::request_response::ReqProtocolNames,
	selendra_overseer::BlockInfo,
	sp_core::traits::SpawnNamed,
	sp_trie::PrefixedMemoryDB,
};

use selendra_node_subsystem_util::database::Database;

#[cfg(feature = "full-node")]
pub use {
	relay_chain_selection::SelectRelayChain,
	sc_client_api::AuxStore,
	selendra_overseer::{Handle, Overseer, OverseerConnector, OverseerHandle},
	selendra_primitives::runtime_api::ParachainHost,
	sp_authority_discovery::AuthorityDiscoveryApi,
	sp_blockchain::{HeaderBackend, HeaderMetadata},
	sp_consensus_babe::BabeApi,
};

#[cfg(feature = "full-node")]
use selendra_node_subsystem::jaeger;

use std::{sync::Arc, time::Duration};

use prometheus_endpoint::Registry;
#[cfg(feature = "full-node")]
use sc_service::KeystoreContainer;
use sc_service::RpcHandlers;
use sc_telemetry::TelemetryWorker;
#[cfg(feature = "full-node")]
use sc_telemetry::{Telemetry, TelemetryWorkerHandle};

#[cfg(feature = "selendra-native")]
pub use selendra_client::SelendraExecutorDispatch;

pub use chain_spec::SelendraChainSpec;
pub use sc_client_api::{Backend, CallExecutor, ExecutionStrategy};
pub use sc_consensus::{BlockImport, LongestChain};
use sc_executor::NativeElseWasmExecutor;
pub use sc_executor::NativeExecutionDispatch;
pub use sc_service::{
	config::{DatabaseSource, PrometheusConfig},
	ChainSpec, Configuration, Error as SubstrateServiceError, PruningMode, Role, RuntimeGenesis,
	TFullBackend, TFullCallExecutor, TFullClient, TaskManager, TransactionPoolOptions,
};
#[cfg(feature = "full-node")]
pub use selendra_client::{
	AbstractClient, Client, ClientHandle, ExecuteWithClient, FullBackend, FullClient,
	RuntimeApiCollection,
};
pub use selendra_primitives::v2::{Block, BlockId, BlockNumber, CollatorPair, Hash, Id as ParaId};
pub use sp_api::{ApiRef, ConstructRuntimeApi, Core as CoreApi, ProvideRuntimeApi, StateBackend};
pub use sp_consensus::{block_validation::Chain, Proposal, SelectChain};
pub use sp_runtime::{
	generic,
	traits::{
		self as runtime_traits, BlakeTwo256, Block as BlockT, HashFor, Header as HeaderT, NumberFor,
	},
};

#[cfg(feature = "selendra-native")]
pub use selendra_runtime;

/// The maximum number of active leaves we forward to the [`Overseer`] on startup.
#[cfg(any(test, feature = "full-node"))]
const MAX_ACTIVE_LEAVES: usize = 4;

/// Provides the header and block number for a hash.
///
/// Decouples `sc_client_api::Backend` and `sp_blockchain::HeaderBackend`.
pub trait HeaderProvider<Block, Error = sp_blockchain::Error>: Send + Sync + 'static
where
	Block: BlockT,
	Error: std::fmt::Debug + Send + Sync + 'static,
{
	/// Obtain the header for a hash.
	fn header(
		&self,
		hash: <Block as BlockT>::Hash,
	) -> Result<Option<<Block as BlockT>::Header>, Error>;
	/// Obtain the block number for a hash.
	fn number(
		&self,
		hash: <Block as BlockT>::Hash,
	) -> Result<Option<<<Block as BlockT>::Header as HeaderT>::Number>, Error>;
}

impl<Block, T> HeaderProvider<Block> for T
where
	Block: BlockT,
	T: sp_blockchain::HeaderBackend<Block> + 'static,
{
	fn header(
		&self,
		hash: Block::Hash,
	) -> sp_blockchain::Result<Option<<Block as BlockT>::Header>> {
		<Self as sp_blockchain::HeaderBackend<Block>>::header(
			self,
			generic::BlockId::<Block>::Hash(hash),
		)
	}
	fn number(
		&self,
		hash: Block::Hash,
	) -> sp_blockchain::Result<Option<<<Block as BlockT>::Header as HeaderT>::Number>> {
		<Self as sp_blockchain::HeaderBackend<Block>>::number(self, hash)
	}
}

/// Decoupling the provider.
///
/// Mandated since `trait HeaderProvider` can only be
/// implemented once for a generic `T`.
pub trait HeaderProviderProvider<Block>: Send + Sync + 'static
where
	Block: BlockT,
{
	type Provider: HeaderProvider<Block> + 'static;

	fn header_provider(&self) -> &Self::Provider;
}

impl<Block, T> HeaderProviderProvider<Block> for T
where
	Block: BlockT,
	T: sc_client_api::Backend<Block> + 'static,
{
	type Provider = <T as sc_client_api::Backend<Block>>::Blockchain;

	fn header_provider(&self) -> &Self::Provider {
		self.blockchain()
	}
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error(transparent)]
	Io(#[from] std::io::Error),

	#[error(transparent)]
	AddrFormatInvalid(#[from] std::net::AddrParseError),

	#[error(transparent)]
	Sub(#[from] SubstrateServiceError),

	#[error(transparent)]
	Blockchain(#[from] sp_blockchain::Error),

	#[error(transparent)]
	Consensus(#[from] sp_consensus::Error),

	#[error("Failed to create an overseer")]
	Overseer(#[from] selendra_overseer::SubsystemError),

	#[error(transparent)]
	Prometheus(#[from] prometheus_endpoint::PrometheusError),

	#[error(transparent)]
	Telemetry(#[from] sc_telemetry::Error),

	#[error(transparent)]
	Jaeger(#[from] selendra_node_subsystem::jaeger::JaegerError),

	#[cfg(feature = "full-node")]
	#[error(transparent)]
	Availability(#[from] AvailabilityError),

	#[error("Authorities require the real overseer implementation")]
	AuthoritiesRequireRealOverseer,

	#[cfg(feature = "full-node")]
	#[error("Creating a custom database is required for validators")]
	DatabasePathRequired,

	#[cfg(feature = "full-node")]
	#[error("Expected at least one of selendra, runtime feature")]
	NoRuntime,
}

/// Can be called for a `Configuration` to identify which network the configuration targets.
pub trait IdentifyVariant {
	/// Returns if this is a configuration for the `Selendra` network.
	fn is_selendra(&self) -> bool;

	/// Returns true if this configuration is for a development network.
	fn is_dev(&self) -> bool;
}

impl IdentifyVariant for Box<dyn ChainSpec> {
	fn is_selendra(&self) -> bool {
		self.id().starts_with("selendra") || self.id().starts_with("sel")
	}

	fn is_dev(&self) -> bool {
		self.id().ends_with("dev")
	}
}

#[cfg(feature = "full-node")]
fn open_database(db_source: &DatabaseSource) -> Result<Arc<dyn Database>, Error> {
	let parachains_db = match db_source {
		DatabaseSource::RocksDb { path, .. } => parachains_db::open_creating_rocksdb(
			path.clone(),
			parachains_db::CacheSizes::default(),
		)?,
		DatabaseSource::ParityDb { path, .. } => parachains_db::open_creating_paritydb(
			path.parent().ok_or(Error::DatabasePathRequired)?.into(),
			parachains_db::CacheSizes::default(),
		)?,
		DatabaseSource::Auto { paritydb_path, rocksdb_path, .. } =>
			if paritydb_path.is_dir() && paritydb_path.exists() {
				parachains_db::open_creating_paritydb(
					paritydb_path.parent().ok_or(Error::DatabasePathRequired)?.into(),
					parachains_db::CacheSizes::default(),
				)?
			} else {
				parachains_db::open_creating_rocksdb(
					rocksdb_path.clone(),
					parachains_db::CacheSizes::default(),
				)?
			},
		DatabaseSource::Custom { .. } => {
			unimplemented!("No selendra subsystem db for custom source.");
		},
	};
	Ok(parachains_db)
}

/// Initialize the `Jeager` collector. The destination must listen
/// on the given address and port for `UDP` packets.
#[cfg(any(test, feature = "full-node"))]
fn jaeger_launch_collector_with_agent(
	spawner: impl SpawnNamed,
	config: &Configuration,
	agent: Option<std::net::SocketAddr>,
) -> Result<(), Error> {
	if let Some(agent) = agent {
		let cfg = jaeger::JaegerConfig::builder()
			.agent(agent)
			.named(&config.network.node_name)
			.build();

		jaeger::Jaeger::new(cfg).launch(spawner)?;
	}
	Ok(())
}

#[cfg(feature = "full-node")]
type FullSelectChain = relay_chain_selection::SelectRelayChain<FullBackend>;
#[cfg(feature = "full-node")]
type FullGrandpaBlockImport<RuntimeApi, ExecutorDispatch, ChainSelection = FullSelectChain> =
	sc_finality_grandpa::GrandpaBlockImport<
		FullBackend,
		Block,
		FullClient<RuntimeApi, ExecutorDispatch>,
		ChainSelection,
	>;

#[cfg(feature = "full-node")]
type FullBeefyBlockImport<RuntimeApi, ExecutorDispatch, InnerBlockImport> =
	beefy_gadget::import::BeefyBlockImport<
		Block,
		FullBackend,
		FullClient<RuntimeApi, ExecutorDispatch>,
		InnerBlockImport,
	>;

#[cfg(feature = "full-node")]
struct Basics<RuntimeApi, ExecutorDispatch>
where
	RuntimeApi: ConstructRuntimeApi<Block, FullClient<RuntimeApi, ExecutorDispatch>>
		+ Send
		+ Sync
		+ 'static,
	RuntimeApi::RuntimeApi:
		RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<FullBackend, Block>>,
	ExecutorDispatch: NativeExecutionDispatch + 'static,
{
	task_manager: TaskManager,
	client: Arc<FullClient<RuntimeApi, ExecutorDispatch>>,
	backend: Arc<FullBackend>,
	keystore_container: KeystoreContainer,
	telemetry: Option<Telemetry>,
}

#[cfg(feature = "full-node")]
fn new_partial_basics<RuntimeApi, ExecutorDispatch>(
	config: &mut Configuration,
	jaeger_agent: Option<std::net::SocketAddr>,
	telemetry_worker_handle: Option<TelemetryWorkerHandle>,
) -> Result<Basics<RuntimeApi, ExecutorDispatch>, Error>
where
	RuntimeApi: ConstructRuntimeApi<Block, FullClient<RuntimeApi, ExecutorDispatch>>
		+ Send
		+ Sync
		+ 'static,
	RuntimeApi::RuntimeApi:
		RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<FullBackend, Block>>,
	ExecutorDispatch: NativeExecutionDispatch + 'static,
{
	let telemetry = config
		.telemetry_endpoints
		.clone()
		.filter(|x| !x.is_empty())
		.map(move |endpoints| -> Result<_, sc_telemetry::Error> {
			let (worker, mut worker_handle) = if let Some(worker_handle) = telemetry_worker_handle {
				(None, worker_handle)
			} else {
				let worker = TelemetryWorker::new(16)?;
				let worker_handle = worker.handle();
				(Some(worker), worker_handle)
			};
			let telemetry = worker_handle.new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

	let executor = NativeElseWasmExecutor::<ExecutorDispatch>::new(
		config.wasm_method,
		config.default_heap_pages,
		config.max_runtime_instances,
		config.runtime_cache_size,
	);

	let (client, backend, keystore_container, task_manager) =
		sc_service::new_full_parts::<Block, RuntimeApi, _>(
			&config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
		)?;
	let client = Arc::new(client);

	let telemetry = telemetry.map(|(worker, telemetry)| {
		if let Some(worker) = worker {
			task_manager.spawn_handle().spawn(
				"telemetry",
				Some("telemetry"),
				Box::pin(worker.run()),
			);
		}
		telemetry
	});

	jaeger_launch_collector_with_agent(task_manager.spawn_handle(), &*config, jaeger_agent)?;

	Ok(Basics { task_manager, client, backend, keystore_container, telemetry })
}

#[cfg(feature = "full-node")]
fn new_partial<RuntimeApi, ExecutorDispatch, ChainSelection>(
	config: &mut Configuration,
	Basics { task_manager, backend, client, keystore_container, telemetry }: Basics<
		RuntimeApi,
		ExecutorDispatch,
	>,
	select_chain: ChainSelection,
) -> Result<
	sc_service::PartialComponents<
		FullClient<RuntimeApi, ExecutorDispatch>,
		FullBackend,
		ChainSelection,
		sc_consensus::DefaultImportQueue<Block, FullClient<RuntimeApi, ExecutorDispatch>>,
		sc_transaction_pool::FullPool<Block, FullClient<RuntimeApi, ExecutorDispatch>>,
		(
			impl Fn(
				selendra_rpc::DenyUnsafe,
				selendra_rpc::SubscriptionTaskExecutor,
			) -> Result<selendra_rpc::RpcExtension, SubstrateServiceError>,
			(
				sc_consensus_babe::BabeBlockImport<
					Block,
					FullClient<RuntimeApi, ExecutorDispatch>,
					FullBeefyBlockImport<
						RuntimeApi,
						ExecutorDispatch,
						FullGrandpaBlockImport<RuntimeApi, ExecutorDispatch, ChainSelection>,
					>,
				>,
				sc_finality_grandpa::LinkHalf<
					Block,
					FullClient<RuntimeApi, ExecutorDispatch>,
					ChainSelection,
				>,
				sc_consensus_babe::BabeLink<Block>,
				beefy_gadget::BeefyVoterLinks<Block>,
			),
			sc_finality_grandpa::SharedVoterState,
			sp_consensus_babe::SlotDuration,
			Option<Telemetry>,
		),
	>,
	Error,
>
where
	RuntimeApi: ConstructRuntimeApi<Block, FullClient<RuntimeApi, ExecutorDispatch>>
		+ Send
		+ Sync
		+ 'static,
	RuntimeApi::RuntimeApi:
		RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<FullBackend, Block>>,
	ExecutorDispatch: NativeExecutionDispatch + 'static,
	ChainSelection: 'static + SelectChain<Block>,
{
	let transaction_pool = sc_transaction_pool::BasicPool::new_full(
		config.transaction_pool.clone(),
		config.role.is_authority().into(),
		config.prometheus_registry(),
		task_manager.spawn_essential_handle(),
		client.clone(),
	);

	let (grandpa_block_import, grandpa_link) =
		sc_finality_grandpa::block_import_with_authority_set_hard_forks(
			client.clone(),
			&(client.clone() as Arc<_>),
			select_chain.clone(),
			Vec::new(),
			telemetry.as_ref().map(|x| x.handle()),
		)?;

	let justification_import = grandpa_block_import.clone();

	let (beefy_block_import, beefy_voter_links, beefy_rpc_links) =
		beefy_gadget::beefy_block_import_and_links(
			grandpa_block_import,
			backend.clone(),
			client.clone(),
		);

	let babe_config = sc_consensus_babe::Config::get(&*client)?;
	let (block_import, babe_link) =
		sc_consensus_babe::block_import(babe_config.clone(), beefy_block_import, client.clone())?;

	let slot_duration = babe_link.config().slot_duration();
	let import_queue = sc_consensus_babe::import_queue(
		babe_link.clone(),
		block_import.clone(),
		Some(Box::new(justification_import)),
		client.clone(),
		select_chain.clone(),
		move |_, ()| async move {
			let timestamp = sp_timestamp::InherentDataProvider::from_system_time();

			let slot =
				sp_consensus_babe::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
					*timestamp,
					slot_duration,
				);

			Ok((timestamp, slot))
		},
		&task_manager.spawn_essential_handle(),
		config.prometheus_registry(),
		sp_consensus::CanAuthorWithNativeVersion::new(client.executor().clone()),
		telemetry.as_ref().map(|x| x.handle()),
	)?;

	let justification_stream = grandpa_link.justification_stream();
	let shared_authority_set = grandpa_link.shared_authority_set().clone();
	let shared_voter_state = sc_finality_grandpa::SharedVoterState::empty();
	let finality_proof_provider = GrandpaFinalityProofProvider::new_for_service(
		backend.clone(),
		Some(shared_authority_set.clone()),
	);

	let shared_epoch_changes = babe_link.epoch_changes().clone();
	let slot_duration = babe_config.slot_duration();

	let import_setup = (block_import, grandpa_link, babe_link, beefy_voter_links);
	let rpc_setup = shared_voter_state.clone();

	let rpc_extensions_builder = {
		let client = client.clone();
		let keystore = keystore_container.sync_keystore();
		let transaction_pool = transaction_pool.clone();
		let select_chain = select_chain.clone();
		let chain_spec = config.chain_spec.cloned_box();
		let backend = backend.clone();

		move |deny_unsafe,
		      subscription_executor: selendra_rpc::SubscriptionTaskExecutor|
		      -> Result<selendra_rpc::RpcExtension, sc_service::Error> {
			let deps = selendra_rpc::FullDeps {
				client: client.clone(),
				pool: transaction_pool.clone(),
				select_chain: select_chain.clone(),
				chain_spec: chain_spec.cloned_box(),
				deny_unsafe,
				babe: selendra_rpc::BabeDeps {
					babe_config: babe_config.clone(),
					shared_epoch_changes: shared_epoch_changes.clone(),
					keystore: keystore.clone(),
				},
				grandpa: selendra_rpc::GrandpaDeps {
					shared_voter_state: shared_voter_state.clone(),
					shared_authority_set: shared_authority_set.clone(),
					justification_stream: justification_stream.clone(),
					subscription_executor: subscription_executor.clone(),
					finality_provider: finality_proof_provider.clone(),
				},
				beefy: selendra_rpc::BeefyDeps {
					beefy_finality_proof_stream: beefy_rpc_links.from_voter_justif_stream.clone(),
					beefy_best_block_stream: beefy_rpc_links.from_voter_best_beefy_stream.clone(),
					subscription_executor,
				},
			};

			selendra_rpc::create_full(deps, backend.clone()).map_err(Into::into)
		}
	};

	Ok(sc_service::PartialComponents {
		client,
		backend,
		task_manager,
		keystore_container,
		select_chain,
		import_queue,
		transaction_pool,
		other: (rpc_extensions_builder, import_setup, rpc_setup, slot_duration, telemetry),
	})
}

#[cfg(feature = "full-node")]
pub struct NewFull<C> {
	pub task_manager: TaskManager,
	pub client: C,
	pub overseer_handle: Option<Handle>,
	pub network: Arc<sc_network::NetworkService<Block, <Block as BlockT>::Hash>>,
	pub rpc_handlers: RpcHandlers,
	pub backend: Arc<FullBackend>,
}

#[cfg(feature = "full-node")]
impl<C> NewFull<C> {
	/// Convert the client type using the given `func`.
	pub fn with_client<NC>(self, func: impl FnOnce(C) -> NC) -> NewFull<NC> {
		NewFull {
			client: func(self.client),
			task_manager: self.task_manager,
			overseer_handle: self.overseer_handle,
			network: self.network,
			rpc_handlers: self.rpc_handlers,
			backend: self.backend,
		}
	}
}

/// Is this node a collator?
#[cfg(feature = "full-node")]
#[derive(Clone)]
pub enum IsCollator {
	/// This node is a collator.
	Yes(CollatorPair),
	/// This node is not a collator.
	No,
}

#[cfg(feature = "full-node")]
impl std::fmt::Debug for IsCollator {
	fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
		use sp_core::Pair;
		match self {
			IsCollator::Yes(pair) => write!(fmt, "Yes({})", pair.public()),
			IsCollator::No => write!(fmt, "No"),
		}
	}
}

#[cfg(feature = "full-node")]
impl IsCollator {
	/// Is this a collator?
	fn is_collator(&self) -> bool {
		matches!(self, Self::Yes(_))
	}
}

/// Returns the active leaves the overseer should start with.
#[cfg(feature = "full-node")]
async fn active_leaves<RuntimeApi, ExecutorDispatch>(
	select_chain: &impl SelectChain<Block>,
	client: &FullClient<RuntimeApi, ExecutorDispatch>,
) -> Result<Vec<BlockInfo>, Error>
where
	RuntimeApi: ConstructRuntimeApi<Block, FullClient<RuntimeApi, ExecutorDispatch>>
		+ Send
		+ Sync
		+ 'static,
	RuntimeApi::RuntimeApi:
		RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<FullBackend, Block>>,
	ExecutorDispatch: NativeExecutionDispatch + 'static,
{
	let best_block = select_chain.best_chain().await?;

	let mut leaves = select_chain
		.leaves()
		.await
		.unwrap_or_default()
		.into_iter()
		.filter_map(|hash| {
			let number = HeaderBackend::number(client, hash).ok()??;

			// Only consider leaves that are in maximum an uncle of the best block.
			if number < best_block.number().saturating_sub(1) {
				return None
			} else if hash == best_block.hash() {
				return None
			};

			let parent_hash = client.header(&BlockId::Hash(hash)).ok()??.parent_hash;

			Some(BlockInfo { hash, parent_hash, number })
		})
		.collect::<Vec<_>>();

	// Sort by block number and get the maximum number of leaves
	leaves.sort_by_key(|b| b.number);

	leaves.push(BlockInfo {
		hash: best_block.hash(),
		parent_hash: *best_block.parent_hash(),
		number: *best_block.number(),
	});

	Ok(leaves.into_iter().rev().take(MAX_ACTIVE_LEAVES).collect())
}

/// Create a new full node of arbitrary runtime and executor.
///
/// This is an advanced feature and not recommended for general use. Generally, `build_full` is
/// a better choice.
///
/// `overseer_enable_anyways` always enables the overseer, based on the provided `OverseerGenerator`,
/// regardless of the role the node has. The relay chain selection (longest or disputes-aware) is
/// still determined based on the role of the node. Likewise for authority discovery.
#[cfg(feature = "full-node")]
pub fn new_full<RuntimeApi, ExecutorDispatch, OverseerGenerator>(
	mut config: Configuration,
	is_collator: IsCollator,
	grandpa_pause: Option<(u32, u32)>,
	enable_beefy: bool,
	jaeger_agent: Option<std::net::SocketAddr>,
	telemetry_worker_handle: Option<TelemetryWorkerHandle>,
	program_path: Option<std::path::PathBuf>,
	overseer_enable_anyways: bool,
	overseer_gen: OverseerGenerator,
	overseer_message_channel_capacity_override: Option<usize>,
	_malus_finality_delay: Option<u32>,
	hwbench: Option<sc_sysinfo::HwBench>,
) -> Result<NewFull<Arc<FullClient<RuntimeApi, ExecutorDispatch>>>, Error>
where
	RuntimeApi: ConstructRuntimeApi<Block, FullClient<RuntimeApi, ExecutorDispatch>>
		+ Send
		+ Sync
		+ 'static,
	RuntimeApi::RuntimeApi:
		RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<FullBackend, Block>>,
	ExecutorDispatch: NativeExecutionDispatch + 'static,
	OverseerGenerator: OverseerGen,
{
	use selendra_node_network_protocol::request_response::IncomingRequest;

	let role = config.role.clone();
	let force_authoring = config.force_authoring;
	let backoff_authoring_blocks = {
		let backoff = sc_consensus_slots::BackoffAuthoringOnFinalizedHeadLagging::default();
		Some(backoff)
	};

	// If not on a known mainnet, warn the user that BEEFY is still experimental.
	if enable_beefy && config.chain_spec.is_selendra() {
		gum::warn!("BEEFY is still experimental, usage on a production network is discouraged.");
	}

	let disable_grandpa = config.disable_grandpa;
	let name = config.network.node_name.clone();

	let basics = new_partial_basics::<RuntimeApi, ExecutorDispatch>(
		&mut config,
		jaeger_agent,
		telemetry_worker_handle,
	)?;

	let prometheus_registry = config.prometheus_registry().cloned();

	let overseer_connector = OverseerConnector::default();
	let overseer_handle = Handle::new(overseer_connector.handle());

	let local_keystore = basics.keystore_container.local_keystore();
	let auth_or_collator = role.is_authority() || is_collator.is_collator();
	let requires_overseer_for_chain_sel = local_keystore.is_some() && auth_or_collator;

	let pvf_checker_enabled = false;

	let select_chain = if requires_overseer_for_chain_sel {
		let metrics =
			selendra_node_subsystem_util::metrics::Metrics::register(prometheus_registry.as_ref())?;

		SelectRelayChain::new_with_overseer(
			basics.backend.clone(),
			overseer_handle.clone(),
			metrics,
		)
	} else {
		SelectRelayChain::new_longest_chain(basics.backend.clone())
	};

	let sc_service::PartialComponents::<_, _, SelectRelayChain<_>, _, _, _> {
		client,
		backend,
		mut task_manager,
		keystore_container,
		select_chain,
		import_queue,
		transaction_pool,
		other: (rpc_extensions_builder, import_setup, rpc_setup, slot_duration, mut telemetry),
	} = new_partial::<RuntimeApi, ExecutorDispatch, SelectRelayChain<_>>(
		&mut config,
		basics,
		select_chain,
	)?;

	let shared_voter_state = rpc_setup;
	let auth_disc_publish_non_global_ips = config.network.allow_non_globals_in_dht;

	let genesis_hash = client.block_hash(0).ok().flatten().expect("Genesis block exists; qed");

	// Note: GrandPa is pushed before the Selendra-specific protocols. This doesn't change
	// anything in terms of behaviour, but makes the logs more consistent with the other
	// Substrate nodes.
	let grandpa_protocol_name =
		sc_finality_grandpa::protocol_standard_name(&genesis_hash, &config.chain_spec);
	config
		.network
		.extra_sets
		.push(sc_finality_grandpa::grandpa_peers_set_config(grandpa_protocol_name.clone()));

	let beefy_protocol_name =
		beefy_gadget::protocol_standard_name(&genesis_hash, &config.chain_spec);

	if enable_beefy {
		config
			.network
			.extra_sets
			.push(beefy_gadget::beefy_peers_set_config(beefy_protocol_name.clone()));
	}

	{
		use selendra_network_bridge::{peer_sets_info, IsAuthority};
		let is_authority = if role.is_authority() { IsAuthority::Yes } else { IsAuthority::No };
		config.network.extra_sets.extend(peer_sets_info(is_authority));
	}

	let req_protocol_names = ReqProtocolNames::new(&genesis_hash, config.chain_spec.fork_id());
	let (pov_req_receiver, cfg) = IncomingRequest::get_config_receiver(&req_protocol_names);
	config.network.request_response_protocols.push(cfg);
	let (chunk_req_receiver, cfg) = IncomingRequest::get_config_receiver(&req_protocol_names);
	config.network.request_response_protocols.push(cfg);
	let (collation_req_receiver, cfg) = IncomingRequest::get_config_receiver(&req_protocol_names);
	config.network.request_response_protocols.push(cfg);
	let (available_data_req_receiver, cfg) =
		IncomingRequest::get_config_receiver(&req_protocol_names);
	config.network.request_response_protocols.push(cfg);
	let (statement_req_receiver, cfg) = IncomingRequest::get_config_receiver(&req_protocol_names);
	config.network.request_response_protocols.push(cfg);
	let (dispute_req_receiver, cfg) = IncomingRequest::get_config_receiver(&req_protocol_names);
	config.network.request_response_protocols.push(cfg);

	let warp_sync = Arc::new(sc_finality_grandpa::warp_proof::NetworkProvider::new(
		backend.clone(),
		import_setup.1.shared_authority_set().clone(),
		Vec::new(),
	));

	let (network, system_rpc_tx, network_starter) =
		sc_service::build_network(sc_service::BuildNetworkParams {
			config: &config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			spawn_handle: task_manager.spawn_handle(),
			import_queue,
			block_announce_validator_builder: None,
			warp_sync: Some(warp_sync),
		})?;

	if config.offchain_worker.enabled {
		let offchain_workers = Arc::new(sc_offchain::OffchainWorkers::new_with_options(
			client.clone(),
			sc_offchain::OffchainWorkerOptions { enable_http_requests: false },
		));

		// Start the offchain workers to have
		task_manager.spawn_handle().spawn(
			"offchain-notifications",
			None,
			sc_offchain::notification_future(
				config.role.is_authority(),
				client.clone(),
				offchain_workers,
				task_manager.spawn_handle().clone(),
				network.clone(),
			),
		);
	}

	let parachains_db = open_database(&config.database)?;

	let availability_config = AvailabilityConfig {
		col_data: parachains_db::REAL_COLUMNS.col_availability_data,
		col_meta: parachains_db::REAL_COLUMNS.col_availability_meta,
	};

	let approval_voting_config = ApprovalVotingConfig {
		col_data: parachains_db::REAL_COLUMNS.col_approval_data,
		slot_duration_millis: slot_duration.as_millis() as u64,
	};

	let candidate_validation_config = CandidateValidationConfig {
		artifacts_cache_path: config
			.database
			.path()
			.ok_or(Error::DatabasePathRequired)?
			.join("pvf-artifacts"),
		program_path: match program_path {
			None => std::env::current_exe()?,
			Some(p) => p,
		},
	};

	let chain_selection_config = ChainSelectionConfig {
		col_data: parachains_db::REAL_COLUMNS.col_chain_selection_data,
		stagnant_check_interval: Default::default(),
		stagnant_check_mode: chain_selection_subsystem::StagnantCheckMode::PruneOnly,
	};

	let dispute_coordinator_config = DisputeCoordinatorConfig {
		col_data: parachains_db::REAL_COLUMNS.col_dispute_coordinator_data,
	};

	let rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		config,
		backend: backend.clone(),
		client: client.clone(),
		keystore: keystore_container.sync_keystore(),
		network: network.clone(),
		rpc_builder: Box::new(rpc_extensions_builder),
		transaction_pool: transaction_pool.clone(),
		task_manager: &mut task_manager,
		system_rpc_tx,
		telemetry: telemetry.as_mut(),
	})?;

	if let Some(hwbench) = hwbench {
		sc_sysinfo::print_hwbench(&hwbench);

		if let Some(ref mut telemetry) = telemetry {
			let telemetry_handle = telemetry.handle();
			task_manager.spawn_handle().spawn(
				"telemetry_hwbench",
				None,
				sc_sysinfo::initialize_hwbench_telemetry(telemetry_handle, hwbench),
			);
		}
	}

	let (block_import, link_half, babe_link, beefy_links) = import_setup;

	let overseer_client = client.clone();
	let spawner = task_manager.spawn_handle();
	// Cannot use the `RelayChainSelection`, since that'd require a setup _and running_ overseer
	// which we are about to setup.
	let active_leaves =
		futures::executor::block_on(active_leaves(select_chain.as_longest_chain(), &*client))?;

	let authority_discovery_service = if auth_or_collator || overseer_enable_anyways {
		use futures::StreamExt;
		use sc_network::Event;
		use sc_network_common::service::NetworkEventStream;

		let authority_discovery_role = if role.is_authority() {
			sc_authority_discovery::Role::PublishAndDiscover(keystore_container.keystore())
		} else {
			// don't publish our addresses when we're not an authority (collator, forests, ..)
			sc_authority_discovery::Role::Discover
		};
		let dht_event_stream =
			network.event_stream("authority-discovery").filter_map(|e| async move {
				match e {
					Event::Dht(e) => Some(e),
					_ => None,
				}
			});
		let (worker, service) = sc_authority_discovery::new_worker_and_service_with_config(
			sc_authority_discovery::WorkerConfig {
				publish_non_global_ips: auth_disc_publish_non_global_ips,
				// Require that authority discovery records are signed.
				strict_record_validation: true,
				..Default::default()
			},
			client.clone(),
			network.clone(),
			Box::pin(dht_event_stream),
			authority_discovery_role,
			prometheus_registry.clone(),
		);

		task_manager.spawn_handle().spawn(
			"authority-discovery-worker",
			Some("authority-discovery"),
			Box::pin(worker.run()),
		);
		Some(service)
	} else {
		None
	};

	if local_keystore.is_none() {
		gum::info!("Cannot run as validator without local keystore.");
	}

	let maybe_params =
		local_keystore.and_then(move |k| authority_discovery_service.map(|a| (a, k)));

	let overseer_handle = if let Some((authority_discovery_service, keystore)) = maybe_params {
		let (overseer, overseer_handle) = overseer_gen
			.generate::<sc_service::SpawnTaskHandle, FullClient<RuntimeApi, ExecutorDispatch>>(
				overseer_connector,
				OverseerGenArgs {
					leaves: active_leaves,
					keystore,
					runtime_client: overseer_client.clone(),
					parachains_db,
					network_service: network.clone(),
					authority_discovery_service,
					pov_req_receiver,
					chunk_req_receiver,
					collation_req_receiver,
					available_data_req_receiver,
					statement_req_receiver,
					dispute_req_receiver,
					registry: prometheus_registry.as_ref(),
					spawner,
					is_collator,
					approval_voting_config,
					availability_config,
					candidate_validation_config,
					chain_selection_config,
					dispute_coordinator_config,
					pvf_checker_enabled,
					overseer_message_channel_capacity_override,
					req_protocol_names,
				},
			)
			.map_err(|e| {
				gum::error!("Failed to init overseer: {}", e);
				e
			})?;
		let handle = Handle::new(overseer_handle.clone());

		{
			let handle = handle.clone();
			task_manager.spawn_essential_handle().spawn_blocking(
				"overseer",
				None,
				Box::pin(async move {
					use futures::{pin_mut, select, FutureExt};

					let forward = selendra_overseer::forward_events(overseer_client, handle);

					let forward = forward.fuse();
					let overseer_fut = overseer.run().fuse();

					pin_mut!(overseer_fut);
					pin_mut!(forward);

					select! {
						_ = forward => (),
						_ = overseer_fut => (),
						complete => (),
					}
				}),
			);
		}
		Some(handle)
	} else {
		assert!(
			!requires_overseer_for_chain_sel,
			"Precondition congruence (false) is guaranteed by manual checking. qed"
		);
		None
	};

	if role.is_authority() {
		let can_author_with =
			sp_consensus::CanAuthorWithNativeVersion::new(client.executor().clone());

		let proposer = sc_basic_authorship::ProposerFactory::new(
			task_manager.spawn_handle(),
			client.clone(),
			transaction_pool,
			prometheus_registry.as_ref(),
			telemetry.as_ref().map(|x| x.handle()),
		);

		let client_clone = client.clone();
		let overseer_handle =
			overseer_handle.as_ref().ok_or(Error::AuthoritiesRequireRealOverseer)?.clone();
		let slot_duration = babe_link.config().slot_duration();
		let babe_config = sc_consensus_babe::BabeParams {
			keystore: keystore_container.sync_keystore(),
			client: client.clone(),
			select_chain,
			block_import,
			env: proposer,
			sync_oracle: network.clone(),
			justification_sync_link: network.clone(),
			create_inherent_data_providers: move |parent, ()| {
				let client_clone = client_clone.clone();
				let overseer_handle = overseer_handle.clone();

				async move {
					let parachain = selendra_node_core_parachains_inherent::ParachainsInherentDataProvider::create(
						&*client_clone,
						overseer_handle,
						parent,
					).await.map_err(|e| Box::new(e))?;

					let timestamp = sp_timestamp::InherentDataProvider::from_system_time();

					let slot =
						sp_consensus_babe::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
							*timestamp,
							slot_duration,
						);

					Ok((timestamp, slot, parachain))
				}
			},
			force_authoring,
			backoff_authoring_blocks,
			babe_link,
			can_author_with,
			block_proposal_slot_portion: sc_consensus_babe::SlotProportion::new(2f32 / 3f32),
			max_block_proposal_slot_portion: None,
			telemetry: telemetry.as_ref().map(|x| x.handle()),
		};

		let babe = sc_consensus_babe::start_babe(babe_config)?;
		task_manager.spawn_essential_handle().spawn_blocking("babe", None, babe);
	}

	// if the node isn't actively participating in consensus then it doesn't
	// need a keystore, regardless of which protocol we use below.
	let keystore_opt =
		if role.is_authority() { Some(keystore_container.sync_keystore()) } else { None };

	if enable_beefy {
		let beefy_params = beefy_gadget::BeefyParams {
			client: client.clone(),
			backend: backend.clone(),
			runtime: client.clone(),
			key_store: keystore_opt.clone(),
			network: network.clone(),
			min_block_delta: 8,
			prometheus_registry: prometheus_registry.clone(),
			protocol_name: beefy_protocol_name,
			links: beefy_links,
		};

		let gadget = beefy_gadget::start_beefy_gadget::<_, _, _, _, _>(beefy_params);

		task_manager.spawn_handle().spawn_blocking("beefy-gadget", None, gadget);
	}

	let gossip_duration = Duration::from_millis(1000);

	let config = sc_finality_grandpa::Config {
		// FIXME substrate#1578 make this available through chainspec
		gossip_duration,
		justification_period: 512,
		name: Some(name),
		observer_enabled: false,
		keystore: keystore_opt,
		local_role: role,
		telemetry: telemetry.as_ref().map(|x| x.handle()),
		protocol_name: grandpa_protocol_name,
	};

	let enable_grandpa = !disable_grandpa;
	if enable_grandpa {
		// start the full GRANDPA voter
		// NOTE: unlike in substrate we are currently running the full
		// GRANDPA voter protocol for all full nodes (regardless of whether
		// they're validators or not). at this point the full voter should
		// provide better guarantees of block and vote data availability than
		// the observer.

		// add a custom voting rule to temporarily stop voting for new blocks
		// after the given pause block is finalized and restarting after the
		// given delay.

		let mut builder = sc_finality_grandpa::VotingRulesBuilder::default();

		#[cfg(not(feature = "malus"))]
		let _malus_finality_delay = None;

		if let Some(delay) = _malus_finality_delay {
			info!(?delay, "Enabling malus finality delay",);
			builder = builder.add(sc_finality_grandpa::BeforeBestBlockBy(delay));
		};

		let voting_rule = match grandpa_pause {
			Some((block, delay)) => {
				info!(
					block_number = %block,
					delay = %delay,
					"GRANDPA scheduled voting pause set for block #{} with a duration of {} blocks.",
					block,
					delay,
				);

				builder.add(grandpa_support::PauseAfterBlockFor(block, delay)).build()
			},
			None => builder.build(),
		};

		let grandpa_config = sc_finality_grandpa::GrandpaParams {
			config,
			link: link_half,
			network: network.clone(),
			voting_rule,
			prometheus_registry: prometheus_registry.clone(),
			shared_voter_state,
			telemetry: telemetry.as_ref().map(|x| x.handle()),
		};

		task_manager.spawn_essential_handle().spawn_blocking(
			"grandpa-voter",
			None,
			sc_finality_grandpa::run_grandpa_voter(grandpa_config)?,
		);
	}

	network_starter.start_network();

	Ok(NewFull { task_manager, client, overseer_handle, network, rpc_handlers, backend })
}

#[cfg(feature = "full-node")]
macro_rules! chain_ops {
	($config:expr, $jaeger_agent:expr, $telemetry_worker_handle:expr; $scope:ident, $executor:ident, $variant:ident) => {{
		let telemetry_worker_handle = $telemetry_worker_handle;
		let jaeger_agent = $jaeger_agent;
		let mut config = $config;
		let basics = new_partial_basics::<$scope::RuntimeApi, $executor>(
			config,
			jaeger_agent,
			telemetry_worker_handle,
		)?;

		use ::sc_consensus::LongestChain;
		// use the longest chain selection, since there is no overseer available
		let chain_selection = LongestChain::new(basics.backend.clone());

		let sc_service::PartialComponents { client, backend, import_queue, task_manager, .. } =
			new_partial::<$scope::RuntimeApi, $executor, LongestChain<_, Block>>(
				&mut config,
				basics,
				chain_selection,
			)?;
		Ok((Arc::new(Client::$variant(client)), backend, import_queue, task_manager))
	}};
}

/// Builds a new object suitable for chain operations.
#[cfg(feature = "full-node")]
pub fn new_chain_ops(
	mut config: &mut Configuration,
	jaeger_agent: Option<std::net::SocketAddr>,
) -> Result<
	(
		Arc<Client>,
		Arc<FullBackend>,
		sc_consensus::BasicQueue<Block, PrefixedMemoryDB<BlakeTwo256>>,
		TaskManager,
	),
	Error,
> {
	config.keystore = sc_service::config::KeystoreConfig::InMemory;

	let telemetry_worker_handle = None;

	#[cfg(feature = "selendra-native")]
	{
		return chain_ops!(config, jaeger_agent, telemetry_worker_handle; selendra_runtime, SelendraExecutorDispatch, Selendra)
	}
	#[cfg(not(feature = "selendra-native"))]
	Err(Error::NoRuntime)
}

/// Build a full node.
///
/// The actual "flavor", aka if it will use `Selendra`, is determined based on
/// [`IdentifyVariant`] using the chain spec.
///
/// `overseer_enable_anyways` always enables the overseer, based on the provided `OverseerGenerator`,
/// regardless of the role the node has. The relay chain selection (longest or disputes-aware) is
/// still determined based on the role of the node. Likewise for authority discovery.
#[cfg(feature = "full-node")]
pub fn build_full(
	config: Configuration,
	is_collator: IsCollator,
	grandpa_pause: Option<(u32, u32)>,
	enable_beefy: bool,
	jaeger_agent: Option<std::net::SocketAddr>,
	telemetry_worker_handle: Option<TelemetryWorkerHandle>,
	overseer_enable_anyways: bool,
	overseer_gen: impl OverseerGen,
	overseer_message_channel_override: Option<usize>,
	malus_finality_delay: Option<u32>,
	hwbench: Option<sc_sysinfo::HwBench>,
) -> Result<NewFull<Client>, Error> {
	#[cfg(feature = "selendra-native")]
	{
		return new_full::<selendra_runtime::RuntimeApi, SelendraExecutorDispatch, _>(
			config,
			is_collator,
			grandpa_pause,
			enable_beefy,
			jaeger_agent,
			telemetry_worker_handle,
			None,
			overseer_enable_anyways,
			overseer_gen,
			overseer_message_channel_override.map(|capacity| {
				gum::warn!("Channel capacity should _never_ be tampered with on selendra!");
				capacity
			}),
			malus_finality_delay,
			hwbench,
		)
		.map(|full| full.with_client(Client::Selendra))
	}

	#[cfg(not(feature = "selendra-native"))]
	Err(Error::NoRuntime)
}

/// Reverts the node state down to at most the last finalized block.
///
/// In particular this reverts:
/// - `ApprovalVotingSubsystem` data in the parachains-db;
/// - `ChainSelectionSubsystem` data in the parachains-db;
/// - Low level Babe and Grandpa consensus data.
#[cfg(feature = "full-node")]
pub fn revert_backend(
	client: Arc<Client>,
	backend: Arc<FullBackend>,
	blocks: BlockNumber,
	config: Configuration,
) -> Result<(), Error> {
	let best_number = client.info().best_number;
	let finalized = client.info().finalized_number;
	let revertible = blocks.min(best_number - finalized);

	if revertible == 0 {
		return Ok(())
	}

	let number = best_number - revertible;
	let hash = client.block_hash_from_id(&BlockId::Number(number))?.ok_or(
		sp_blockchain::Error::Backend(format!(
			"Unexpected hash lookup failure for block number: {}",
			number
		)),
	)?;

	let parachains_db = open_database(&config.database)
		.map_err(|err| sp_blockchain::Error::Backend(err.to_string()))?;

	revert_approval_voting(parachains_db.clone(), hash)?;
	revert_chain_selection(parachains_db, hash)?;
	// Revert Substrate consensus related components
	client.execute_with(RevertConsensus { blocks, backend })?;

	Ok(())
}

fn revert_chain_selection(db: Arc<dyn Database>, hash: Hash) -> sp_blockchain::Result<()> {
	let config = chain_selection_subsystem::Config {
		col_data: parachains_db::REAL_COLUMNS.col_chain_selection_data,
		stagnant_check_interval: chain_selection_subsystem::StagnantCheckInterval::never(),
		stagnant_check_mode: chain_selection_subsystem::StagnantCheckMode::PruneOnly,
	};

	let chain_selection = chain_selection_subsystem::ChainSelectionSubsystem::new(config, db);

	chain_selection
		.revert_to(hash)
		.map_err(|err| sp_blockchain::Error::Backend(err.to_string()))
}

fn revert_approval_voting(db: Arc<dyn Database>, hash: Hash) -> sp_blockchain::Result<()> {
	let config = approval_voting_subsystem::Config {
		col_data: parachains_db::REAL_COLUMNS.col_approval_data,
		slot_duration_millis: Default::default(),
	};

	let approval_voting = approval_voting_subsystem::ApprovalVotingSubsystem::with_config(
		config,
		db,
		Arc::new(sc_keystore::LocalKeystore::in_memory()),
		Box::new(sp_consensus::NoNetwork),
		approval_voting_subsystem::Metrics::default(),
	);

	approval_voting
		.revert_to(hash)
		.map_err(|err| sp_blockchain::Error::Backend(err.to_string()))
}

struct RevertConsensus {
	blocks: BlockNumber,
	backend: Arc<FullBackend>,
}

impl ExecuteWithClient for RevertConsensus {
	type Output = sp_blockchain::Result<()>;

	fn execute_with_client<Client, Api, Backend>(self, client: Arc<Client>) -> Self::Output
	where
		<Api as sp_api::ApiExt<Block>>::StateBackend: sp_api::StateBackend<BlakeTwo256>,
		Backend: sc_client_api::Backend<Block> + 'static,
		Backend::State: sp_api::StateBackend<BlakeTwo256>,
		Api: selendra_client::RuntimeApiCollection<StateBackend = Backend::State>,
		Client: AbstractClient<Block, Backend, Api = Api> + 'static,
	{
		// Revert consensus-related components.
		// The operations are not correlated, thus call order is not relevant.
		sc_consensus_babe::revert(client.clone(), self.backend, self.blocks)?;
		sc_finality_grandpa::revert(client, self.blocks)?;
		Ok(())
	}
}

//! Engine-side runtime configuration.
//!
//! Distinct from `module.toml` (module manifest): this file describes
//! the *engine*'s I/O wiring - chain RPC endpoints and the on-disk
//! location of the `local-store` database. Both are required for the
//! 0.2 reference engine to do anything other than print stubs.
//!
//! Lookup order:
//!
//! 1. `--engine-config <path>` CLI flag (future), or third positional
//!    argument today;
//! 2. `engine.toml` in the current working directory;
//! 3. defaults - no chains configured, `state_dir = ./data`.
//!
//! A missing config is OK for the example module (it only logs); for
//! the cow-api / chain backends it surfaces as `HostError {
//! kind: unsupported }` so guests learn early.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tracing::{info, warn};

/// Engine-side configuration loaded from `engine.toml`.
#[derive(Debug, Default, Deserialize)]
pub struct EngineConfig {
    #[serde(default)]
    pub engine: EngineSection,
    /// Per-chain RPC URLs keyed by EVM chain id (decimal in TOML).
    /// Used by the `chain::request` host call and as the alloy provider
    /// pool seed.
    #[serde(default)]
    pub chains: BTreeMap<u64, ChainConfig>,
}

#[derive(Debug, Deserialize)]
pub struct EngineSection {
    #[serde(default = "default_state_dir")]
    pub state_dir: PathBuf,
    /// `tracing_subscriber::EnvFilter`-compatible directive. Defaults to
    /// `info` when absent; `RUST_LOG` overrides at process start.
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Resource caps applied to every module store at instantiation.
    /// `wasmtime` traps a module that overruns either; the operator
    /// tunes the budget based on their hardware and the modules they
    /// load.
    #[serde(default)]
    pub limits: ModuleLimits,
}

impl Default for EngineSection {
    fn default() -> Self {
        Self {
            state_dir: default_state_dir(),
            log_level: default_log_level(),
            limits: ModuleLimits::default(),
        }
    }
}

/// Per-module resource caps the supervisor applies to each store.
///
/// `wasmtime` exposes two complementary knobs:
///
/// - **Fuel** is decremented per executed instruction (`Config::consume_fuel
///   (true)`). When the store runs out, the module traps with `OutOfFuel`.
///   The budget is reset before every `on_event` invocation so a single
///   greedy event cannot starve the next one.
/// - **Memory size** caps the module's linear memory growth, applied via
///   `StoreLimitsBuilder::memory_size`. A module that requests more linear
///   memory than this gets a `MemoryOutOfBounds`-class trap from
///   `memory.grow`.
///
/// Both are `Option<u64>` so an unset value falls through to the engine's
/// built-in defaults; operators only need to write the keys they actually
/// want to override. `main.rs` reads these at instantiation; the multi-
/// module supervisor (BLEU-818) consumes the same accessors per module
/// store.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct ModuleLimits {
    /// Fuel granted before every `on_event` call. `None` -> engine default.
    #[serde(default)]
    pub fuel_per_event: Option<u64>,
    /// Linear-memory ceiling per module (bytes). `None` -> engine default.
    #[serde(default)]
    pub memory_bytes: Option<u64>,
}

impl ModuleLimits {
    /// Default fuel granted per `on_event` invocation (≈ 1 billion WASM
    /// instructions). Modules that exceed this budget trap with
    /// `OutOfFuel`.
    pub const DEFAULT_FUEL_PER_EVENT: u64 = 1_000_000_000;

    /// Default linear-memory cap per module (64 MiB). Prevents a single
    /// runaway module from exhausting process memory.
    pub const DEFAULT_MEMORY_BYTES: u64 = 64 * 1024 * 1024;

    /// Resolved fuel budget (config override, falling back to the default).
    pub fn fuel(&self) -> u64 {
        self.fuel_per_event.unwrap_or(Self::DEFAULT_FUEL_PER_EVENT)
    }

    /// Resolved memory cap in bytes (config override, falling back to the
    /// default).
    pub fn memory(&self) -> u64 {
        self.memory_bytes.unwrap_or(Self::DEFAULT_MEMORY_BYTES)
    }
}

#[derive(Debug, Deserialize)]
pub struct ChainConfig {
    /// JSON-RPC endpoint. `ws://` and `wss://` engage alloy's pubsub
    /// transport (required for `eth_subscribe`); `http://` and `https://`
    /// engage the HTTP transport (request/response only).
    pub rpc_url: String,
}

fn default_state_dir() -> PathBuf {
    PathBuf::from("./data")
}

fn default_log_level() -> String {
    "info".to_owned()
}

/// Read an engine config from disk, returning defaults if the file is
/// missing. Parse errors propagate.
pub fn load_or_default(path: Option<&Path>) -> anyhow::Result<EngineConfig> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from("engine.toml"),
    };

    if !path.exists() {
        warn!(
            path = %path.display(),
            "engine.toml not found - running with defaults (no chain RPC endpoints; \
             chain::request and cow_api::submit_order will return Unsupported)"
        );
        return Ok(EngineConfig::default());
    }

    let raw = std::fs::read_to_string(&path)?;
    let cfg: EngineConfig = toml::from_str(&raw)?;
    info!(
        path = %path.display(),
        chains = cfg.chains.len(),
        state_dir = %cfg.engine.state_dir.display(),
        "engine config loaded",
    );
    Ok(cfg)
}

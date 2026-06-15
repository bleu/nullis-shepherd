//! Per-instance host state and its WASI view.
//!
//! One [`HostState`] is created per module, lives inside the wasmtime
//! `Store`, and is the receiver every `Host` trait impl in
//! [`super::impls`] is implemented for.

use std::time::Instant;

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use super::cow_orderbook::OrderBookPool;
use super::local_store_redb::ModuleStore;
use super::provider_pool::ProviderPool;

pub(crate) struct HostState {
    pub wasi: WasiCtx,
    pub table: ResourceTable,
    /// Wasmtime memory / table / instance caps applied to this store.
    /// Wired in via `store.limiter(|state| &mut state.limits)` right
    /// after construction; the operator-tunable budget comes from
    /// `engine.toml`'s `[engine.limits]` table.
    pub limits: wasmtime::StoreLimits,
    /// Origin for `clock::monotonic-ns`. Differences between successive
    /// readings are the only meaningful values.
    pub monotonic_baseline: Instant,
    /// Per-module `[capabilities.http].allow` allowlist (from module.toml).
    /// Consulted by `http::fetch` before any outbound call.
    pub http_allowlist: Vec<String>,
    /// Human-readable module name carried by every log line emitted via
    /// the `logging` host. The local-store handle below already encodes
    /// the same identity as its keccak prefix; this field exists purely
    /// for log tagging.
    pub module_namespace: String,
    /// `cow-api` backend - per-chain `OrderBookApi` clients + reqwest.
    pub cow: OrderBookPool,
    /// `chain` backend - per-chain alloy `DynProvider` pool.
    pub chain: ProviderPool,
    /// Per-module local-store handle with the `keccak256(name)` prefix
    /// computed once at instantiation. Every get / set / delete /
    /// list-keys hop is just `prefix ++ key` concat — no per-call
    /// hashing.
    pub store: ModuleStore,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

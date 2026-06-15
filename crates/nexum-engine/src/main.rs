#![cfg_attr(not(test), warn(unused_crate_dependencies))]

// alloy split its API across multiple crates; we depend on the
// transports directly so cargo resolves the right feature set, but
// the runtime code only names them through the `alloy_provider`
// re-exports. Silence `unused_crate_dependencies` with `as _`.
use alloy_rpc_client as _;
use alloy_transport as _;
use alloy_transport_ws as _;

mod bindings;
mod engine_config;
mod host;
mod manifest;

use std::path::PathBuf;
use std::time::Instant;

use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::error::Context as _;
use wasmtime::{Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use crate::bindings::{Config, Shepherd, nexum};
use crate::host::state::HostState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let wasm_path = args.next().ok_or_else(|| {
        anyhow::anyhow!(
            "usage: nexum-engine <path-to-component.wasm> [<module.toml>] [<engine.toml>]"
        )
    })?;
    let explicit_manifest = args.next().map(PathBuf::from);
    let explicit_engine_config = args.next().map(PathBuf::from);

    // -- 1. Load engine config (optional). --
    let engine_cfg = engine_config::load_or_default(explicit_engine_config.as_deref())?;

    // -- 2. Install tracing subscriber. --
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&engine_cfg.engine.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .init();

    info!("nexum-engine starting");
    info!(wasm = %wasm_path, "loading component");

    // -- 3. Load the module manifest. --
    // Canonical name is module.toml (ADR-0001). nexum.toml is accepted with a
    // deprecation warning during the 0.1->0.2 transition; removed in 0.3.
    let manifest_path = explicit_manifest.or_else(|| {
        let dir = PathBuf::from(&wasm_path).parent()?.to_owned();
        let canonical = dir.join("module.toml");
        if canonical.exists() {
            return Some(canonical);
        }
        let legacy = dir.join("nexum.toml");
        if legacy.exists() {
            eprintln!(
                "[deprecation] nexum.toml is deprecated; rename to module.toml (ADR-0001). \
                 Support will be removed in 0.3."
            );
            return Some(legacy);
        }
        None
    });
    let loaded = match manifest_path.as_deref() {
        Some(p) => {
            info!(manifest = %p.display(), "loading module manifest");
            manifest::load(p)?
        }
        None => manifest::fallback_manifest(),
    };

    // -- 4. Bring up the host backends. --
    std::fs::create_dir_all(&engine_cfg.engine.state_dir).with_context(|| {
        format!(
            "create state directory {}",
            engine_cfg.engine.state_dir.display()
        )
    })?;
    let store_path = engine_cfg.engine.state_dir.join("local-store.redb");
    let local_store = host::local_store_redb::LocalStore::open(&store_path)
        .with_context(|| format!("open local-store at {}", store_path.display()))?;
    let cow_pool = host::cow_orderbook::OrderBookPool::default();
    let provider_pool = host::provider_pool::ProviderPool::from_config(&engine_cfg)
        .await
        .context("open chain providers")?;

    // -- 5. Build the wasmtime engine + component. --
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    // Fuel metering is always on so the operator's per-event budget
    // (`engine.toml::[engine.limits].fuel_per_event`) can trap a runaway
    // module before it starves the host. `Store::set_fuel` below seeds
    // the actual budget per instance.
    config.consume_fuel(true);
    // `async_support` was deprecated in wasmtime 45 - the engine
    // resolves async on its own. Keeping the call out of the Config
    // chain silences the `deprecated` warning under
    // `RUSTFLAGS=-D warnings`.
    let engine = Engine::new(&config)?;

    let load_start = Instant::now();
    let component =
        Component::from_file(&engine, &wasm_path).context("failed to load component")?;
    tracing::debug!(elapsed_ms = ?load_start.elapsed(), "component load");

    // Enforce capability declarations before spending time on instantiation.
    manifest::enforce_capabilities(
        &loaded,
        component
            .component_type()
            .imports(&engine)
            .map(|(name, _)| name),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut linker = Linker::<HostState>::new(&engine);
    Shepherd::add_to_linker::<HostState, wasmtime::component::HasSelf<HostState>>(
        &mut linker,
        |state| state,
    )?;
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;

    let wasi = WasiCtxBuilder::new().inherit_stdio().build();
    let module_namespace = if loaded.manifest.module.name.is_empty() {
        "module".to_owned()
    } else {
        loaded.manifest.module.name.clone()
    };
    let module_store = local_store
        .module(&module_namespace)
        .with_context(|| format!("open local-store view for module {module_namespace:?}"))?;

    let limits_cfg = engine_cfg.engine.limits;
    let memory_cap = usize::try_from(limits_cfg.memory()).unwrap_or(usize::MAX);
    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(memory_cap)
        .build();

    let mut store = Store::new(
        &engine,
        HostState {
            wasi,
            table: ResourceTable::new(),
            limits,
            monotonic_baseline: Instant::now(),
            http_allowlist: loaded.http_allowlist,
            module_namespace: module_namespace.clone(),
            cow: cow_pool,
            chain: provider_pool,
            store: module_store,
        },
    );
    store.limiter(|state| &mut state.limits);
    store
        .set_fuel(limits_cfg.fuel())
        .context("seed module fuel budget")?;
    info!(
        fuel = limits_cfg.fuel(),
        memory_bytes = limits_cfg.memory(),
        "applied module resource limits",
    );

    let inst_start = Instant::now();
    let bindings = Shepherd::instantiate_async(&mut store, &component, &linker)
        .await
        .context("failed to instantiate component")?;
    tracing::debug!(elapsed_ms = ?inst_start.elapsed(), "component instantiate");

    info!("calling init");
    let config_entries: Config = if loaded.config.is_empty() {
        vec![("name".into(), loaded.manifest.module.name.clone())]
    } else {
        loaded.config
    };
    let init_start = Instant::now();
    match bindings.call_init(&mut store, &config_entries).await? {
        Ok(()) => info!(elapsed_ms = ?init_start.elapsed(), "init succeeded"),
        Err(e) => warn!(
            domain = %e.domain,
            kind = ?e.kind,
            code = e.code,
            message = %e.message,
            "init failed",
        ),
    }

    // Refuel before on_event so each event runs against a full budget,
    // independent of how much instantiation + init consumed. The
    // supervisor (BLEU-818) does the same per delivered event.
    store
        .set_fuel(limits_cfg.fuel())
        .context("refuel for on_event")?;

    // Dispatch a test block event (timestamps are ms since Unix epoch, UTC).
    info!("dispatching test block event");
    let block = nexum::host::types::Block {
        chain_id: 1,
        number: 19_000_000,
        hash: vec![0xab; 32],
        timestamp: 1_700_000_000_000,
    };
    let event = nexum::host::types::Event::Block(block);
    let evt_start = Instant::now();
    match bindings.call_on_event(&mut store, &event).await? {
        Ok(()) => info!(elapsed_ms = ?evt_start.elapsed(), "on-event succeeded"),
        Err(e) => warn!(
            domain = %e.domain,
            kind = ?e.kind,
            code = e.code,
            message = %e.message,
            "on-event failed",
        ),
    }

    info!("done");
    Ok(())
}

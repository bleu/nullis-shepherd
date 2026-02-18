# Shepherd: Programmable Blockchain Automation

Shepherd is a WASM Component Model runtime that replaces CoW Protocol's hardcoded watch-tower with a programmable, sandboxed execution layer. Developers deploy WebAssembly components ("shepherds") that react to blockchain events, read chain state, submit orders to CoW Protocol, and persist data — all within a secure sandbox with no implicit capabilities.

## Architecture at a Glance

```
                    ┌──────────────────────────────────────────────────────┐
                    │                  Shepherd Runtime                    │
                    │                                                      │
 Module Discovery   │  ┌────────────────────────────────────────────────┐  │
 ─────────────────► │  │            Module Manager                      │  │
 • Static (local)   │  │  Load → Init → Run → Restart → Dead           │  │
 • ENS contenthash  │  │                                                │  │
 • On-chain registry│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐       │  │
                    │  │  │ Module A │ │ Module B │ │ Module C │       │  │
                    │  │  │ (WASM)   │ │ (WASM)   │ │ (WASM)   │       │  │
                    │  │  └────┬─────┘ └────┬─────┘ └────┬─────┘       │  │
                    │  └───────┼────────────┼────────────┼──────────────┘  │
                    │          │            │            │                  │
                    │  ┌───────▼────────────▼────────────▼──────────────┐  │
                    │  │              Host API (WIT)                    │  │
                    │  │  rpc · cow · state · order · logging            │  │
                    │  └───────┬────────────┬────────────┬──────────────┘  │
                    │          │            │            │                  │
                    │   ┌──────▼──────┐ ┌──▼───┐ ┌──────▼──────┐          │
                    │   │ alloy (RPC) │ │ redb │ │ CoW API     │          │
                    │   └──────┬──────┘ └──────┘ └─────────────┘          │
                    │          │                                            │
                    │  ┌───────▼────────────────────────────────────────┐  │
                    │  │           Event Source Manager                 │  │
                    │  │  Block subscribers · Log watchers · Cron       │  │
                    │  └───────────────────────────────────────────────-┘  │
                    │                                                      │
                    │  ┌────────────────────────────────────────────────┐  │
                    │  │           Observability                        │  │
                    │  │  tracing (JSON logs) · metrics (Prometheus)    │  │
                    │  │  /health endpoint · CLI                        │  │
                    │  └────────────────────────────────────────────────┘  │
                    └──────────────────────────────────────────────────────┘
```

## Design Principles

- **Component Model from day 1** — WIT-defined API contract; structural sandboxing (no WASI, no FS, no network); multi-language guests.
- **Declarative subscriptions** — modules declare events in their manifest; the runtime wires sources.
- **Transactional state** — per-event all-or-nothing semantics; commit on success, rollback on trap.
- **Content-addressed distribution** — modules are fetched by hash (Swarm, IPFS, OCI, HTTPS); integrity always verified.
- **Self-hosted** — no centralised dependency; operator runs their own node.

## Technology Stack

| Concern | Choice | Version |
|---------|--------|---------|
| Language | Rust | 1.90+ |
| WASM runtime | wasmtime (Component Model) | 41.x |
| API contract | WIT (`shepherd:core@0.1.0`) | — |
| Guest bindings | wit-bindgen | 0.53.x |
| Async | Tokio | — |
| Ethereum RPC | alloy | 1.5.x |
| State store | redb | 3.1.x |
| Logging | tracing + tracing-subscriber | — |
| Metrics | metrics + metrics-exporter-prometheus | — |
| Deployment | Docker | — |
| License | AGPL-3.0 | — |

## WIT World (API Surface)

The `shepherd-module` world is the single contract between host and guest. Modules import host capabilities and export lifecycle hooks:

```
world shepherd-module {
    import rpc            — generic JSON-RPC passthrough (eth_* and beyond)
    import cow            — CoW Protocol REST API access
    import state          — get, set, delete, list_keys
    import order          — submit
    import logging        — log (trace/debug/info/warn/error)

    export init(config)   — called once on load
    export on_event(event)— called per subscribed event (block, logs, timer)
}
```

No WASI interfaces are imported. All I/O is mediated through these five interfaces. The `rpc` interface exposes a single generic `request` function — the SDK implements alloy's `Transport` trait on top of it, giving modules the full alloy `Provider` API (80+ methods) with zero WIT churn.

> Design rationale: [07-rpc-namespace-design.md](07-rpc-namespace-design.md)

→ Full WIT definition: [01-runtime-environment.md](01-runtime-environment.md)

## Module Package

A module ships as a **bundle**: a manifest (`shepherd.toml`) plus a compiled WASM component.

```toml
# shepherd.toml
[module]
name = "twap-monitor"
version = "0.2.0"
wasm = "sha256:9f86d081…"       # content hash of module.wasm

[module.resources]
max_memory_bytes = 10_485_760    # 10 MB
max_fuel_per_event = 100_000
max_state_bytes = 52_428_800     # 50 MB

[chains]
required = [42161]               # must have RPC for these chains

[[subscribe]]
type = "block"
chain_id = 42161

[config]
cow_api_url = "https://api.cow.fi/arbitrum"
```

The manifest declares identity, resource caps, chain requirements, event subscriptions, and opaque module config — everything the runtime needs to load and run the module.

→ Full spec: [02-modules-events-packaging.md](02-modules-events-packaging.md)

## Module Discovery

Three layers, from simplest to most decentralised:

| Method | How it works |
|--------|-------------|
| **Static** | Operator points at a local manifest path |
| **ENS** | Module author sets ENS `contenthash` (ENSIP-7) to a Swarm/IPFS reference; runtime resolves and fetches |
| **On-chain registry** | Runtime watches contract events or ENS `TextChanged` events for module registrations |

All methods converge: resolve content reference → fetch via content store → verify hash → load.

→ Full design: [03-module-discovery.md](03-module-discovery.md)

## Module Lifecycle

```
Resolve → Load → Init → Run ⇄ Restart → Dead
```

- **Resolve**: fetch WASM by content hash from Swarm/IPFS/OCI/local.
- **Load**: compile `Component`, validate WIT world, create `InstancePre`.
- **Init**: create `Store`, instantiate, call `init(config)`.
- **Run**: dispatch subscribed events to `on_event`. Each call gets a fuel budget.
- **Restart**: on crash — exponential backoff (1s → 5min cap), fresh `Store`, state persists.
- **Dead**: after N consecutive failures (poison pill) — requires manual intervention.

→ Full lifecycle: [02-modules-events-packaging.md](02-modules-events-packaging.md)

## Event System

- **Sources**: `block` (new heads via `eth_subscribe`), `log` (filtered contract events), `cron` (schedule-based).
- **Shared subscriptions**: one block subscription per chain, fanned out to all subscribed modules.
- **Dispatch**: concurrent across modules, sequential within a module (ordered delivery).
- **Declared in manifest**: `[[subscribe]]` blocks — the runtime wires sources, not the module.

→ Full design: [02-modules-events-packaging.md](02-modules-events-packaging.md)

## State Store

- **Backend**: redb (pure Rust, ACID, MVCC, crash-safe).
- **Isolation**: one database file per module; modules cannot access each other's state.
- **Transactions**: each `on_event` runs in an implicit write transaction — commit on success, rollback on failure.
- **Survives restarts**: state is external to WASM instance.
- **Size enforcement**: `max_state_bytes` from manifest, enforced host-side.
- **Prefix scanning**: `list_keys(prefix)` for namespaced key organisation.

→ Full design: [04-state-store.md](04-state-store.md)

## SDK (`shepherd-sdk`)

The SDK is the only dependency a module author needs:

| Layer | Provides |
|-------|----------|
| `#[shepherd::module]` | Proc macro — eliminates WIT/bindgen boilerplate; supports `async fn` for natural `.await` |
| `provider(chain_id)` | Full alloy `Provider` backed by host RPC (via `HostTransport`) |
| `CowClient` | Typed CoW Protocol API client backed by host `cow` interface |
| `prelude::*` | All types, interfaces, helpers in one import |
| `TypedState` | Serde-based typed state (postcard serialisation) |
| `abi::sol!` | Compile-time Ethereum ABI codec (alloy-sol-types) |
| `log::{info!, …}` | Formatted logging macros |
| `Error` / `Result` | Proper error type with `?` support |
| `testing::MockHost` | Native-Rust unit tests with mock host |
| `testing::WasmTestHarness` | Integration tests in real wasmtime |
| `cargo shepherd` | CLI: new / build / package / publish |

Multi-language support: module authors can use Rust, C/C++, Go, JavaScript, or Python — all compile to valid components against the same WIT world.

→ Full design: [05-sdk-design.md](05-sdk-design.md)

## Production Hardening

### Resource Enforcement

| Resource | Mechanism | On breach |
|----------|-----------|-----------|
| CPU (deterministic) | Fuel | Trap → rollback → restart |
| CPU (wall-clock) | Epoch interruption | Yield to Tokio |
| Memory | `ResourceLimiter` | `memory.grow` denied |
| Storage | Host-side tracking | `state::set` returns `Err` |

### RPC Resilience

Tower layer stack per chain: timeout → retry (exponential + jitter) → rate limit → fallback endpoint. WebSocket subscriptions auto-reconnect with missed-block backfill.

### Observability

| Signal | Stack | Endpoint |
|--------|-------|----------|
| Logs | `tracing` → JSON | stdout |
| Metrics | `metrics` → Prometheus | `:9090/metrics` |
| Health | HTTP JSON | `:8080/health` |

Metrics cover three groups: runtime-level (modules loaded/dead), per-module (events, latency, fuel, restarts, state usage), per-chain RPC (requests, errors, fallbacks, blocks behind).

→ Full design: [06-production-hardening.md](06-production-hardening.md)

## Grant Milestones

| # | Milestone | Effort | Key Deliverables |
|---|-----------|--------|------------------|
| 1 | Core Runtime & Event System | 120h | wasmtime Component Model host, WIT interfaces, event sources, redb state store, CLI |
| 2 | TWAP & Ethflow Modules | 100h | TWAP monitor, Ethflow monitor, ComposableCoW contract mods |
| 3 | SDK & Developer Experience | 60h | `shepherd-sdk` crate, proc macro, testing framework, examples, docs |
| 4 | Production Hardening | 60h | Resource limits, restart policy, logging, metrics, health checks |
| 5 | Multi-Chain & Deployment | 40h | Multi-chain config, Docker image, deployment docs |

## Repository Structure

```
shepherd/
├── crates/
│   ├── runtime/           Core WASM host, event system, state store
│   ├── sdk/               shepherd-sdk crate for module authors
│   ├── cli/               shepherd operator CLI (run, module, state)
│   └── cargo-shepherd/    cargo subcommand for module authors (new, build, package, publish)
├── modules/
│   ├── twap-monitor/      TWAP order monitoring module
│   └── ethflow-watcher/   Ethflow order monitoring module
├── wit/
│   └── shepherd.wit       Canonical WIT definition (single source of truth; SDK references via path)
├── docker/
│   └── Dockerfile
└── docs/
    ├── 00-overview.md
    ├── 01-runtime-environment.md
    ├── 02-modules-events-packaging.md
    ├── 03-module-discovery.md
    ├── 04-state-store.md
    ├── 05-sdk-design.md
    ├── 06-production-hardening.md
    └── 07-rpc-namespace-design.md
```

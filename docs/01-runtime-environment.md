# Runtime Environment: wasmtime + Component Model

## Version Target

**wasmtime 41.x** (latest stable as of Feb 2026).

- Release cadence: new major on the 20th of each month.
- LTS every 12th version (24 months support). Nearest LTS: v36.
- Requires **Rust 1.90.0+**.
- Repo: https://github.com/bytecodealliance/wasmtime

## Why wasmtime

| Criterion | wasmtime | wasmer | wasm3 |
|-----------|----------|--------|-------|
| Rust-native embedding | First-class | Yes | C FFI |
| Async host functions | Yes | No | No |
| Component Model / WASI | Full | Partial | No |
| Fuel / epoch metering | Both | Fuel only | Injection |
| Production users | Fastly, Fermyon, Cloudflare, Zed | General | Embedded |
| Sandboxing | Proven | Similar | Similar |

## Decision: Component Model from Day 1

### Rationale

The Component Model is **production-viable in wasmtime 41** and gives us critical advantages over raw core modules:

1. **Structural sandboxing.** A component compiled against a WIT world with no filesystem import literally *cannot* access the filesystem — enforced at the type level, not just by omission of host functions. This is stronger than core module sandboxing where imports are stringly-typed.

2. **Type-safe API contract.** The WIT definition *is* the API spec. Both host and guest get generated bindings (`wasmtime::component::bindgen!` on the host, `wit_bindgen::generate!` on the guest). No manual ABI wrangling, no serialisation disagreements.

3. **Resource types.** Opaque handles with lifecycle management (constructors, methods, destructors via `ResourceTable`). Ideal for subscription handles, RPC connections, etc.

4. **Multi-language guests from day 1.** Module authors can use Rust, C/C++, Go, JavaScript (ComponentizeJS), or Python (componentize-py) — all producing valid components against the same WIT world. This dramatically lowers the barrier for community modules.

5. **No WASI required.** The Component Model and WASI are architecturally separate. We define a pure `shepherd:core` world with exactly our host APIs. Zero WASI imports means zero implicit capabilities.

6. **Acceptable overhead.** The canonical ABI adds marshalling for strings/lists (memory copy across boundary), but for a plugin system with coarse-grained calls this is negligible. `InstancePre` front-loads validation costs.

### What we give up

- **Tooling churn.** `wit-bindgen` (v0.53) and `cargo-component` (v0.21) are functional but APIs are not yet stable. Pin versions in the SDK.
- **Native async Component Model** (`stream<T>`, `future<T>`) is still evolving (v41 had breaking changes to the async canonical ABI). We use basic async host functions (`func_wrap_async`) which are stable.

### Risk assessment

| Aspect | Risk |
|--------|------|
| `bindgen!` macro, custom worlds, resource types | Low — stable, well-documented |
| `wit-bindgen` guest bindings | Medium — API churn between versions |
| Component Model native async (streams/futures) | High — not needed yet, avoid for now |

## Core Concepts

### Engine

Global, thread-safe compilation environment. One per process.

```rust
let mut config = Config::new();
config.async_support(true);
config.consume_fuel(true);
config.epoch_interruption(true);
let engine = Engine::new(&config)?;
```

### Store

Per-module execution context. Holds component instances, host state (`ShepherdHostState`), fuel counters, resource limits, and the `ResourceTable` for handle management.

```rust
let mut store = Store::new(&engine, ShepherdHostState {
    table: ResourceTable::new(),
    rpc: alloy_provider,
    db: redb_handle,
    // ...
});
store.set_fuel(10_000)?;
store.epoch_deadline_async_yield_and_update(10); // yield after 10 epochs (~1s at 100ms tick)
```

### Component → InstancePre → Instance

1. **Component**: compiled from `.wasm` component binary (expensive, cacheable, thread-safe).
2. **Linker**: binds host implementations of our WIT interfaces.
3. **InstancePre**: pre-validated component + linker (reusable across stores).
4. **Instance**: a live component in a specific store, from which we call exports.

```rust
let component = Component::from_file(&engine, "twap_monitor.wasm")?;
let mut linker = Linker::new(&engine);
ShepherdModule::add_to_linker(&mut linker, |state| state)?;

// Pre-validate once, instantiate many times (one per store)
let pre = linker.instantiate_pre(&component)?;
let bindings = ShepherdModule::instantiate_pre(&mut store, &pre)?;
```

## WIT World: `shepherd:core`

The WIT definition is the single source of truth for the host↔guest contract. Shepherd defines a custom world with **no WASI imports**:

```wit
package shepherd:core@0.1.0;

interface types {
    type chain-id = u64;

    record block-data {
        chain-id: chain-id,
        number: u64,
        hash: list<u8>,
        timestamp: u64,
    }

    record log-entry {
        chain-id: chain-id,
        address: list<u8>,
        topics: list<list<u8>>,
        data: list<u8>,
        block-number: u64,
        tx-hash: list<u8>,
        log-index: u32,
    }

    variant event {
        block(block-data),
        logs(list<log-entry>),
        timer(u64),
    }

    /// Opaque config from shepherd.toml [config] section.
    type config = list<tuple<string, string>>;
}

interface csn {
    use types.{chain-id};

    /// JSON-RPC error returned by the provider or the host.
    record json-rpc-error {
        code: s64,
        message: string,
        data: option<string>,
    }

    /// Execute a JSON-RPC request against the specified chain.
    ///
    /// The host forwards the request to the configured alloy provider for
    /// the given chain, applying timeout/retry/rate-limit/fallback middleware
    /// transparently. Method includes the namespace prefix (e.g. "eth_call").
    ///
    /// `params` and the success return value are JSON-encoded strings matching
    /// the JSON-RPC spec. The host handles id/jsonrpc framing; the guest only
    /// provides method + params and receives the `result` field.
    ///
    /// See doc 07 (RPC Namespace Design) for the full design rationale: a
    /// single generic function replaces per-method WIT functions, enabling
    /// the SDK to implement alloy's Transport trait and expose the full
    /// alloy Provider API (80+ methods) to guest modules with zero WIT churn.
    request: func(chain-id: chain-id, method: string, params: string)
        -> result<string, json-rpc-error>;
}

interface cow {
    use types.{chain-id};

    record api-error {
        status: u16,
        message: string,
        body: option<string>,
    }

    /// HTTP-style request to the CoW Protocol API.
    ///
    /// The host routes to the correct CoW API base URL for the given chain.
    /// `method`: "GET" | "POST" | "PUT" | "DELETE"
    /// `path`: relative API path, e.g. "/api/v1/orders"
    /// `body`: optional JSON request body
    request: func(
        chain-id: chain-id,
        method: string,
        path: string,
        body: option<string>,
    ) -> result<string, api-error>;
}

interface state {
    get: func(key: string) -> result<option<list<u8>>, string>;
    set: func(key: string, value: list<u8>) -> result<_, string>;
    delete: func(key: string) -> result<_, string>;
    list-keys: func(prefix: string) -> result<list<string>, string>;
}

interface order {
    use types.{chain-id};

    submit: func(chain-id: chain-id, order-data: list<u8>)
        -> result<string, string>;
}

interface logging {
    enum level { trace, debug, info, warn, error }
    log: func(level: level, message: string);
}

world shepherd-module {
    import csn;
    import cow;
    import state;
    import order;
    import logging;

    /// Called once on load. Receives config from shepherd.toml.
    export init: func(config: types.config) -> result<_, string>;

    /// Called for each subscribed event.
    export on-event: func(event: types.event) -> result<_, string>;
}
```

Key properties:
- **No WASI** — modules cannot access FS, network, clocks, or random.
- **All I/O through our interfaces** — RPC reads, CoW API, state, order submission, logging.
- **Generic JSON-RPC passthrough** — the `csn` interface exposes a single `request` function. The SDK implements alloy's `Transport` trait on top of it, giving modules the full alloy `Provider` API. See doc 07 for details.
- **`list<u8>` for raw bytes** — state values, order payloads, etc. The SDK provides typed wrappers.
- **Resource types** can be added later (e.g. subscription handles, cursor-based log iteration).

## Host-Side Embedding

The host uses `wasmtime::component::bindgen!` to generate Rust traits from the WIT:

```rust
wasmtime::component::bindgen!({
    path: "wit/shepherd.wit",
    world: "shepherd-module",
    async: true,
});

impl shepherd::core::csn::Host for ShepherdHostState {
    async fn request(
        &mut self,
        chain_id: u64,
        method: String,
        params: String,
    ) -> Result<Result<String, JsonRpcError>> {
        if !self.is_method_allowed(&method) {
            return Ok(Err(JsonRpcError {
                code: -32601,
                message: format!("method not allowed: {method}"),
                data: None,
            }));
        }

        let provider = self.provider_for(chain_id)?;
        let raw_params: Box<RawValue> = RawValue::from_string(params)?;

        // One function handles the entire eth_ namespace — alloy's provider
        // stack (timeout, retry, rate-limit, fallback) applies transparently.
        match provider.raw_request_dyn(method.into(), &raw_params).await {
            Ok(result) => Ok(Ok(result.get().to_string())),
            Err(e) => Ok(Err(e.into())),
        }
    }
}

impl shepherd::core::state::Host for ShepherdHostState {
    async fn get(&mut self, key: String) -> Result<Result<Option<Vec<u8>>, String>> {
        // Read from the in-flight WriteTransaction (not a new ReadTransaction)
        // so the module sees its own uncommitted writes within a single on_event.
        let table = self.write_txn.open_table(self.state_table())?;
        Ok(Ok(table.get(key.as_str())?.map(|v| v.value().to_vec())))
    }
    // ...
}
```

See doc 07 for the full `csn` and `cow` host implementations, method allowlisting, and the `HostTransport` that bridges this to alloy's `Provider` API on the guest side.

## Guest-Side (Module Author) Experience

Module authors add the `shepherd-sdk` crate and use the `#[shepherd::module]` proc macro. The macro provides **named event handlers** (`on_block`, `on_logs`, `on_timer`) — it generates the `on_event` match dispatch, WIT export wrapper, and optional provider injection. Handlers can be `async fn` for natural `.await`:

```rust
use shepherd_sdk::prelude::*;

sol! {
    function getTradeableOrderWithSignature(
        address owner, bytes32 ctx, bytes32 orderHash
    ) external view returns (bytes memory order, bytes memory signature);
}

#[shepherd::module]
struct TwapMonitor;

impl TwapMonitor {
    fn init(config: Config) -> Result<()> {
        info!("TWAP monitor starting");
        Ok(())
    }

    // Named handler — macro generates on_event match dispatch.
    // provider is injected from block.chain_id.
    // async fn — macro wraps in block_on (single-poll, zero overhead).
    async fn on_block(block: BlockData, provider: &RootProvider) -> Result<()> {
        // Full alloy Provider API — natural .await
        let block_num = provider.get_block_number().await?;
        let balance = provider.get_balance(owner).latest().await?;

        // Typed contract calls with sol! + EthCall builder
        let tx = TransactionRequest::default()
            .to(contract)
            .input(getTradeableOrderWithSignatureCall {
                owner, ctx, orderHash: order_hash,
            }.abi_encode().into());
        let result = provider.call(tx).latest().await?;
        let decoded = getTradeableOrderWithSignatureCall::abi_decode_returns(&result)?;

        // CoW API via typed client
        let cow = CowClient::new(block.chain_id);
        cow.submit_order(&order)?;

        // State persistence
        TypedState::set("last_block", &block_num)?;
        Ok(())
    }

    // Only define handlers for events you subscribe to.
    // No on_logs or on_timer → those events are silently ignored.
}
```

Build with `cargo component build --release` (or `cargo build --target wasm32-wasip2` + `wasm-tools component new`).

See doc 05 for the full macro design (named handlers, provider injection, escape hatch) and doc 07 for the `HostTransport` implementation and `provider()` constructor.

## Multi-Language Guest Support

| Language | Tooling | Maturity |
|----------|---------|----------|
| **Rust** | `wit-bindgen` + `cargo-component` | Mature |
| **C/C++** | `wit-bindgen c` + WASI SDK | Mature |
| **Go** | `wit-bindgen` Go generator | Maturing |
| **JavaScript** | ComponentizeJS (SpiderMonkey) | Maturing |
| **Python** | componentize-py (CPython) | Maturing |
| **C#** | `wit-bindgen-csharp` | Emerging |

All produce valid components against the same `shepherd-module` world.

## Execution Metering

### Fuel (deterministic cost accounting)

- `Config::consume_fuel(true)` — each WASM op consumes fuel; exhaustion traps.
- Use for **per-invocation budgets**: cap a single `on_event` callback.

### Epoch Interruption (cooperative time-slicing)

- `Config::epoch_interruption(true)` — background Tokio task calls `engine.increment_epoch()` on a fixed interval.
- Stores yield at epoch boundaries via `epoch_deadline_async_yield_and_update`.
- Use for **wall-clock fairness**: prevent one module from starving others.

Both are needed: fuel for correctness, epochs for liveness.

## Resource Limits

Implement `ResourceLimiter` to cap per-module:

- **Memory growth** — target <10 MB default.
- **Table growth** — max entries.
- **Instance count** — max concurrent.

Enforced synchronously on every `memory.grow` / `table.grow`.

## Async Integration

All RPC and CoW API I/O is async (alloy / reqwest on the host). wasmtime bridges this:

- `Config::async_support(true)`.
- Host functions registered with `func_wrap_async` (or via `async: true` in `bindgen!`).
- Guest exports called with `call_async`.
- wasmtime runs WASM on a separate native stack; `Future::poll` drives execution.
- Epoch yielding ensures cooperation with the Tokio scheduler.

**Note:** We use wasmtime's basic async support (stable), *not* the Component Model native async (`stream<T>`, `future<T>`) which is still evolving.

## WASI: Intentionally Excluded (for now)

- WASI 0.2.1 is stable in wasmtime. WASI 0.3 (native async) is in preview.
- Shepherd's `shepherd-module` world imports **zero WASI interfaces**.
- This is a security feature: components structurally cannot access FS/network/clocks.
- If a future use case needs selective WASI (e.g. `wasi:clocks` for timing), we can define an extended world:

```wit
world shepherd-module-extended {
    include shepherd-module;
    import wasi:clocks/monotonic-clock@0.2.0;
}
```

The host only adds WASI to the linker for modules that request it — capability-based.

## Summary: Shepherd ↔ wasmtime Mapping

| Shepherd Concept | wasmtime Primitive |
|------------------|--------------------|
| Runtime process | `Engine` (one, shared) |
| API contract | WIT world (`shepherd-module`) |
| Compiled module | `Component` (cached, thread-safe) |
| Pre-validated module | `InstancePre` (linker + component) |
| Running instance | `Store<ShepherdHostState>` + `Instance` |
| Host API impl | Traits generated by `bindgen!` |
| Opaque handles | `Resource<T>` + `ResourceTable` |
| Per-call budget | Fuel |
| Wall-clock fairness | Epoch interruption |
| Memory/table caps | `ResourceLimiter` |
| Async RPC / CoW I/O | `func_wrap_async` + Tokio |
| Persistent state | redb (per-module database file, via `state` interface host fns) |

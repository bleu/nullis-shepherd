---
status: proposed
---

# Dynamic address registration for log subscriptions (Envio HyperIndex-style)

## Context

Some module archetypes need to track contracts deployed dynamically by a factory — Uniswap V3 pools (deployed by `UniswapV3Factory`), Aave V3 reserves (registered via `PoolAddressesProvider`), lending market deployments, NFT marketplace collections. Static `[[subscription]]` declarations in `nexum.toml` cannot express this: the child addresses are not known when the module's manifest is authored.

Neither TWAP nor EthFlow (the M2 grant deliverables) needs this — both subscribe to a single well-known contract per chain (`ComposableCoW`, `CoWSwapEthFlow`). This ADR is forward-looking: 0.2 is the breaking-change window per `docs/migration/0.1-to-0.2.md` §522, with contracts stable from 0.2.0 onwards. Adding factory support after 0.2 would require another major version bump.

Two production EVM indexer frameworks define the design space:

- **Ponder** (`ponder.config.ts`) is fully declarative: the user describes the factory contract, the factory event, and which event parameter holds the child address. Ponder extracts the address itself and indexes children automatically. The framework does ABI-aware extraction; the user writes no factory handler.
- **Envio HyperIndex** (`envio.yaml` + handler code) is a hybrid: the user declares the **template** (ABI, event topics) without an address, then calls `context.<Contract>.register(address)` inside the factory event handler. Topics are static; the watched address set grows at runtime.

The Envio model decouples "what topics the engine listens for" (static, in the manifest) from "which addresses are interesting" (dynamic, driven by module code). The engine maintains a single aggregated subscription per template; the address set is mutated as the module learns of new contracts.

## Decision

Adopt the Envio model. Two pieces:

**1. Manifest schema gains `[[subscription.template]]`** — a topic-only log subscription whose address set is populated at runtime:

```toml
[[subscription.template]]
chain_id = 1
name = "uniswap_v3_pool"
event_topics = [
    "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",   # Swap
    "0x7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",   # Mint
]
# Optional: pre-register a set of addresses at boot (before init runs).
# Useful for protocols where some addresses are known statically.
initial_addresses = []
```

Existing `[[subscription]]` blocks with concrete `address` are unchanged. A module typically declares one static `[[subscription]]` for the factory event itself, plus one `[[subscription.template]]` per child contract type.

**2. `nexum:host/chain` gains two host functions** for the module to manage the address set:

```wit
interface chain {
    // ... existing request, request-batch, subscribe-blocks, subscribe-logs ...

    /// Add an address to the watch set for `template-name` on `chain-id`.
    /// If `from-block` is set and precedes the current head, the engine
    /// runs paginated `eth_getLogs` over the template's topics on this
    /// address from `from-block` to head before going live. Registration
    /// is idempotent — calling twice with the same arguments is a no-op.
    register-address: func(
        chain-id: chain-id,
        template-name: string,
        address: list<u8>,            // 20 bytes
        from-block: option<u64>,
    ) -> result<_, host-error>;

    /// Remove an address from the watch set. Subsequent events on that
    /// address are dropped. Idempotent — unregistering an unknown address
    /// returns Ok.
    unregister-address: func(
        chain-id: chain-id,
        template-name: string,
        address: list<u8>,
    ) -> result<_, host-error>;
}
```

**3. `log-source` variant in `nexum:host/event-module`** gains a `template` case so modules can route events:

```wit
variant log-source {
    static(u32),               // existing — index into [[subscription]]
    template(string),          // new — name of the [[subscription.template]]
}
```

Module code (Rust, Uniswap V3 indexer):

```rust
fn init(config: Vec<(String, String)>) -> Result<(), HostError> {
    // Resume: re-register every pool we already discovered. Each pool's
    // creation block is persisted; backfill resumes from where we left off.
    for key in local_store::list_keys("pool:")? {
        let pool = parse_addr(&key);
        let from_block = u64::from_le_bytes(local_store::get(&key)?.unwrap());
        chain::register_address(1, "uniswap_v3_pool", &pool, Some(from_block))?;
    }
    Ok(())
}

fn on_event(event: Event) -> Result<(), HostError> {
    match event {
        // Factory event — declared as a static [[subscription]] for the factory.
        Event::Log(LogEvent { log, source: LogSource::Static(0) }) => {
            let pool = decode_pool_created(&log)?;
            chain::register_address(1, "uniswap_v3_pool", &pool, Some(log.block_number))?;
            local_store::set(&format!("pool:{}", hex(&pool)), &log.block_number.to_le_bytes())?;
        }
        // Pool event — dispatched through the template.
        Event::Log(LogEvent { log, source: LogSource::Template(name) })
            if name == "uniswap_v3_pool" =>
        {
            process_pool_event(log)?;
        }
        _ => {}
    }
    Ok(())
}
```

Engine internals:

- **Boot**: read all `[[subscription.template]]` blocks across loaded modules. Initialise per-template address sets from `initial_addresses` plus the reserved key `__nexum:template:{name}:addresses` in the module's `local-store` namespace.
- **Live**: one `eth_subscribe logs` per chain per template, filter = `(topic in event_topics) ∧ (address in current_set)`. When `register-address` mutates the set, the engine re-subscribes (or shards the filter when the set exceeds the provider's address-limit threshold).
- **Backfill on register**: if `from-block` < current head, run paginated `eth_getLogs(topic, address, from-block, head)` synchronously before joining the live stream. Events from backfill are dispatched through the same `on_event` callback as live events.
- **Persistence**: engine writes `__nexum:template:{name}:addresses` (one entry per registered address) and `__nexum:template:{name}:cursor:{address}` (last block dispatched per address) in the module's reserved namespace. Resume is automatic if the module re-calls `register-address` for each persisted address; engine deduplicates via the persisted cursor.

## Considered options

- **Ponder full-declarative** (factory address + event + parameter declared in manifest, engine extracts child). Rejected: schema must express ABI-aware extraction (`child = { source = "topic", index = 1 }` or `{ source = "data", offset = 32, length = 20 }`), and grows with every exotic factory shape (nested factories, multi-child events, address computed from multiple fields). The Envio model pushes that complexity into module Rust code — where it belongs, given the module already decodes events with `alloy_sol_types`.
- **Pure imperative `chain.open-log-stream(filter) -> stream-handle`.** Rejected: each call opens a new subscription, so N pools = N WSS connections. Doesn't scale to indexers with 10k+ tracked addresses. The Envio model keeps one subscription per template and mutates its address set — engine batches naturally.
- **Wildcard manifest** (`address = "*"` with topic-only filter, module client-side filters). Rejected: mainnet has ~10k contracts emitting `Transfer` / `Swap` per day. The engine would deliver every matching event to every wildcard subscriber; modules pay fuel and bandwidth to discard 99% of them.
- **Defer factory pattern entirely to 0.3.** Rejected: 0.2 is the breaking-change window per migration:522. Adding either `[[subscription.template]]` or `register-address` after 0.2 requires another major bump. Better to land the small surface now than break later.
- **Templates declared inside `[[subscription]]` with optional `address` (one block, two modes).** Rejected: conflates two semantically distinct cases — modules looking at `[[subscription]]` would have to inspect for the presence of `address` to know whether they need to call `register-address`. Separate block name is clearer.
- **Engine extracts the child address from the static factory `[[subscription]]` (best of both).** Rejected: would require the manifest to identify a relationship between the static subscription and a template, plus the ABI extraction rules. Reintroduces the Ponder schema complexity we just rejected.

## Consequences

- `nexum.toml` schema gains `[[subscription.template]]` with `chain_id`, `name`, `event_topics`, optional `initial_addresses`. mfw78 approval needed for the schema extension (his spec).
- `nexum:host/chain` gains `register-address` and `unregister-address` functions; `nexum:host/event-module`'s `log-source` variant gains `template(string)`. mfw78 approval needed for the WIT change (his namespace). These are the only WIT additions in this ADR — small, focused, with no implicit dependencies on other interfaces.
- Reserved key namespace `__nexum:template:*` in each module's `local-store` namespace. Modules MUST NOT write to keys with this prefix; engine reserves them.
- Module boilerplate per factory ≈ 5–10 lines (decode the factory event, call `register-address`, persist for resume). The M3 SDK is expected to ship a helper that encapsulates this — something like `Factory::<PoolCreated>::on_event(register_template("uniswap_v3_pool"))` — but the host surface is intentionally simple enough that no SDK is required to use it.
- Engine can register addresses sourced from anywhere — factory events, HTTP API responses, governance votes, operator-supplied lists. Composability is a deliberate feature; the engine treats every registration the same way regardless of provenance.
- Nested factories (a child contract that is itself a factory) work without schema changes: the child's event handler decodes its own creation events and calls `register-address` on the grandchild template. Engine has no concept of nesting; it just multiplexes addresses per template.
- Conditional registration ("only register pools with fee = 3000") works without schema changes: the module's factory-event handler inspects the event payload and decides whether to call `register-address`.
- Backfill cost: a module registering 10k addresses with `from-block` deep in history triggers 10k paginated `eth_getLogs` runs, sequentially per address (the engine cannot batch across addresses with different `from-block` cursors without state-machine work that's not in scope for 0.2). Operators should set sensible `start_block` boundaries; the M3 SDK is expected to ship a `BulkBackfill` helper that groups same-cursor addresses into combined filter queries.
- The address set per template is bounded only by `local-store` quota. The engine enforces a soft cap (default 50k addresses per template) configurable in `engine.toml` to prevent a runaway module from saturating the provider's filter limits; exceeding the cap returns `host-error.denied` from `register-address`.
- A module that never calls `register-address` for a declared template receives no events from it — equivalent to declaring an unused subscription. Engine logs a warning on boot if a template is declared with `initial_addresses = []` and the module has no registrations after `init` returns.

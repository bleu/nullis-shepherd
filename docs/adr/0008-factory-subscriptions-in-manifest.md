---
status: proposed
---

# Dynamic address registration for log subscriptions

## Context

Some module archetypes need to track contracts deployed dynamically by a factory, for example Uniswap V3 pools (deployed by `UniswapV3Factory`). Static `[[subscription]]` declarations in `nexum.toml` cannot express this: the child addresses are not known when the module's manifest is authored.

Neither TWAP nor EthFlow (the M2 grant deliverables) needs this. Both subscribe to a single well-known contract per chain. This ADR is forward-looking, motivated by `docs/migration/0.1-to-0.2.md` §522 declaring 0.2 the breaking-change window; adding factory support after 0.2 would require another major version bump.

Envio HyperIndex uses a hybrid pattern that fits Shepherd's design: topics are declared statically in the manifest, and the watched address set is mutated at runtime via a `register()` host call. The engine maintains a single aggregated log subscription per template; the address set grows as the module learns of new contracts.

Whether the engine should also handle historical backfill on register (the module passes `from-block`, engine paginates `eth_getLogs` from there to head before going live) is a separate decision flagged for upstream review. This ADR keeps the engine surface minimal and defers historical replay to the existing module-driven catch-up pattern documented in `docs/02:260`.

## Decision

Two pieces:

**1. Manifest schema gains `[[subscription.template]]`**, a topic-only log subscription whose address set is populated at runtime:

```toml
[[subscription.template]]
chain_id = 1
name = "uniswap_v3_pool"
event_topics = [
    "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",   # Swap
    "0x7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",   # Mint
]
```

Existing `[[subscription]]` blocks with concrete `address` are unchanged. A module typically declares one static `[[subscription]]` for the factory event itself, plus one `[[subscription.template]]` per child contract type.

**2. `nexum:host/chain` gains two host functions** for the module to manage the address set:

```wit
interface chain {
    // existing request, request-batch, subscribe-blocks, subscribe-logs ...

    /// Add an address to the watch set for `template-name` on `chain-id`.
    /// Idempotent: calling twice with the same arguments is a no-op.
    register-address: func(
        chain-id: chain-id,
        template-name: string,
        address: list<u8>,            // 20 bytes
    ) -> result<_, host-error>;

    /// Remove an address from the watch set. Subsequent events on that
    /// address are dropped. Idempotent.
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
    static(u32),               // existing: index into [[subscription]]
    template(string),          // new: name of the [[subscription.template]]
}
```

Module code (Rust, Uniswap V3 indexer):

```rust
fn init(config: Vec<(String, String)>) -> Result<(), HostError> {
    // Resume: re-register every pool we already discovered.
    for key in local_store::list_keys("pool:")? {
        let pool = parse_addr(&key);
        chain::register_address(1, "uniswap_v3_pool", &pool)?;
    }
    Ok(())
}

fn on_event(event: Event) -> Result<(), HostError> {
    match event {
        // Factory event, declared as a static [[subscription]] for the factory.
        Event::Log(LogEvent { log, source: LogSource::Static(0) }) => {
            let pool = decode_pool_created(&log)?;
            chain::register_address(1, "uniswap_v3_pool", &pool)?;
            local_store::set(&format!("pool:{}", hex(&pool)), b"")?;
        }
        // Pool event, dispatched through the template.
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

- **Boot**: read all `[[subscription.template]]` blocks. Initialise per-template address sets from the reserved key `__nexum:template:{name}:addresses` in the module's `local-store` namespace if present from a prior run.
- **Live**: one `eth_subscribe logs` per chain per template with filter `(topic in event_topics) AND (address in current_set)`. When `register-address` mutates the set, the engine re-subscribes.
- **Persistence**: engine writes `__nexum:template:{name}:addresses` whenever the set changes. Resume after restart is automatic if the module re-calls `register-address` in `init`.

Historical backfill is the module's responsibility, consistent with the catch-up pattern documented in `docs/02:260`. The module calls `chain.request("eth_getLogs", ...)` during `init` to replay history before going live. The engine does not backfill on `register-address`.

## Considered options

- **Ponder full-declarative** (factory address, event, parameter declared in manifest; engine extracts child). Rejected: schema must express ABI-aware extraction (`child = { source = "topic", index = 1 }` or similar), and grows with every exotic factory shape (nested factories, multi-child events, address computed from multiple fields). The Envio model pushes that complexity into module Rust code, where it belongs given the module already decodes events with `alloy_sol_types`.
- **Pure imperative `chain.open-log-stream(filter) -> stream-handle`.** Rejected: each call opens a new subscription, so N pools means N WSS connections. Doesn't scale to indexers with 10k+ tracked addresses. The Envio model keeps one subscription per template and mutates its address set; engine batches naturally.
- **Engine-driven historical backfill on register** (with `from-block` parameter). Rejected after PR review flagged the added complexity. Module-driven catch-up via `init` + `eth_getLogs` already exists in mfw's design (`docs/02:260`) and covers the same use case without adding engine state (per-address cursor, paginated `eth_getLogs` orchestration). M3 SDK can ship a helper that wraps the pattern.
- **Wildcard manifest** (`address = "*"` with topic-only filter, module client-side filters). Rejected: mainnet has ~10k contracts emitting `Transfer` or `Swap` per day. The engine would deliver every matching event to every wildcard subscriber; modules pay fuel and bandwidth to discard 99% of them.
- **Defer factory pattern entirely to 0.3.** Rejected: 0.2 is the breaking-change window per migration:522. Adding either `[[subscription.template]]` or `register-address` after 0.2 requires another major bump.
- **Templates declared inside `[[subscription]]` with optional `address` (one block, two modes).** Rejected: conflates two semantically distinct cases. Modules looking at `[[subscription]]` would have to inspect for the presence of `address` to know whether they need to call `register-address`. Separate block name is clearer.

## Consequences

- `nexum.toml` schema gains `[[subscription.template]]` with `chain_id`, `name`, `event_topics`. Schema extension needs upstream approval.
- `nexum:host/chain` gains `register-address` and `unregister-address`; `nexum:host/event-module`'s `log-source` variant gains `template(string)`. WIT change needs upstream approval.
- Reserved key namespace `__nexum:template:*` in each module's `local-store` namespace. Modules MUST NOT write to keys with this prefix.
- Module boilerplate per factory is roughly 5 lines (decode the factory event, call `register-address`, persist for resume). The M3 SDK can ship a helper that wraps it.
- Register sources are not limited to factory events. A module can register addresses from any signal: HTTP API responses, governance votes, operator-supplied lists. Composability is a deliberate feature.
- Nested factories (a child contract that is itself a factory) work without schema changes. The child's event handler calls `register-address` on the grandchild template.
- Conditional registration ("only register pools with fee = 3000") works without schema changes. The module's factory-event handler decides.
- The address set per template is bounded only by `local-store` quota. The engine enforces a soft cap (default 50k addresses per template) configurable in `engine.toml`; exceeding the cap returns `host-error.denied` from `register-address`.
- Open follow-up: whether to support `from-block` historical backfill on register is left for upstream discussion. The minimal surface here can be extended additively if needed.

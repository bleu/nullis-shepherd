---
status: deferred
deferred-to: 0.3
---

# Dynamic address registration for log subscriptions (deferred to 0.3)

## Status

**Deferred to 0.3.** Neither TWAP nor EthFlow (the M2 grant deliverables) needs this capability, and the design's complexity is not justified by current need.

This ADR is preserved as a reference for the design space; the final shape will be revisited when the first module actually requiring dynamic address registration emerges.

## Context

Some module archetypes need to track contracts deployed dynamically by a factory, for example Uniswap V3 pools (deployed by `UniswapV3Factory`). Static `[[subscription]]` declarations in `module.toml` cannot express this: the child addresses are not known when the module's manifest is authored.

Neither TWAP nor EthFlow needs this; both subscribe to a single well-known contract per chain. This ADR was originally framed as forward-looking work to land in 0.2's breaking-change window.

## Why deferred

Two considerations motivate the deferral:

1. **`eth_getLogs` already supports topic-only filtering.** The JSON-RPC method accepts a filter without an `address` field, so a module subscribing to a topic across all addresses can be served by the existing primitives if the operator's RPC endpoint cooperates. If topic-only filters at the JSON-RPC layer are good enough for the common case, the engine does not need a manifest-and-host-function mechanism on top.
2. **The schema and host-function surface add engine complexity that no M2 deliverable consumes.** The historical-backfill story is the largest contributor to that complexity and was already trimmed once; deferring the rest in the same spirit avoids paying for a mechanism nothing exercises yet.

Combined: the dynamic-subscription design is not load-bearing for M2 deliverables, and the simplest path (topic-only `eth_subscribe` filters with module-side address filtering) may suffice for a wide range of indexer use cases. The dynamic-registration mechanism originally proposed (Envio-style `register-address`) addresses scaling concerns at high address counts but should land when a real consumer is on the table to validate the trade-off.

## Reference design (not adopted in 0.2)

The original proposal - kept here so future discussions have a starting point - was a hybrid of static topics and dynamic addresses:

- `[[subscription.template]]` block in `module.toml` declaring `chain_id`, `name`, `event_topics` (no address).
- `chain.register-address(chain_id, template_name, address)` host function for the module to add addresses at runtime.
- `chain.unregister-address(chain_id, template_name, address)` mirror function.
- `log-source.template(string)` variant on the event dispatch so modules route by template name.
- Engine maintains a single aggregated `eth_subscribe logs` per chain per template, with filter `(topic ∈ event_topics) ∧ (address ∈ current_set)`. The address set is mutated as the module discovers new contracts.
- Historical backfill (`from-block` argument on register, paginated `eth_getLogs` orchestration) was contentious and was already trimmed before deferral.

Envio HyperIndex's `context.<Contract>.register()` API is the closest existing pattern, validated in production for indexers tracking thousands of dynamically-discovered contracts.

## Alternatives left open for 0.3

- **Topic-only `[[subscription]]`** (no address field; engine forwards `eth_subscribe logs` with topic-only filter; module client-side filters logs by address it cares about). Simplest, no new host functions. Trade-off: firehose volume for common topics like `Transfer`.
- **Dynamic register-address** (the original reference design above).
- **Engine-extracted factory child addresses** (Ponder-style declarative schema with ABI-aware extraction rules). Schema complexity grows with exotic factory shapes.
- **No factory pattern; modules wanting dynamic discovery use raw `chain.subscribe-logs` with topic-only filter and persist the discovered address set themselves**.

The choice depends on what the first consumer actually needs.

## Consequences of deferring

- The `shepherd:cow` and `nexum:host` WIT surfaces remain unchanged in 0.2.
- `module.toml` schema does not gain `[[subscription.template]]` in 0.2.
- 0.2 is the breaking-change window; adding any of the above options in 0.3 may require a major version bump if the chosen shape extends `module.toml` or `nexum:host/chain` non-additively. This risk is accepted on the basis that the M2 grant deliverables do not require this surface.
- TWAP and EthFlow modules ship in 0.2 against the existing static `[[subscription]]` declarations (one address per subscription, known at manifest authorship time). This is consistent with how the autopilot ethflow indexer and watch-tower configure their subscriptions today.

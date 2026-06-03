---
status: proposed
---

# TWAP and EthFlow as intent helpers in `shepherd:cow@0.2.0`

## Context

The reference engine already exposes `shepherd:cow/cow-api` for raw orderbook access (REST passthrough + `submit-order`). Two further CoW workflows show up in every non-trivial module: ComposableCoW conditional orders (TWAP being the canonical example) and EthFlow native-ETH orders. Both follow the same external-indexer/relayer pattern that CoW maintainers have signalled intent to extract from the monolithic `cowprotocol/services` repository:

- **TWAP / ComposableCoW** is already extracted as the standalone `cowprotocol/watch-tower` (TypeScript). Listens to `ConditionalOrderCreated`, polls `getTradeableOrderWithSignature` on each block, posts to the orderbook when an order becomes tradeable.
- **EthFlow indexer** still lives inside `cowprotocol/services/crates/autopilot/src/database/onchain_order_events/ethflow_events.rs`. Listens to `OrderPlacement` / `OrderInvalidation` / `OrderRefund`, inserts into the `ethflow_orders` table. The intent is to extract it into a standalone service following the same path `watch-tower` and the `refunder` crate already took. The Shepherd `ethflow-watcher` module is positioned as that extraction.

Both flows share the same pattern: observe an on-chain event, derive a signed `OrderCreation`, submit it to the orderbook. The derivation has enough protocol detail (signing scheme, ComposableCoW eth_call, EthFlow EIP-1271 contract signature, log decoding) that a guest module would either ship that logic itself (large WASM, duplicates work in the `cowprotocol` Rust SDK) or make ten round-trips to the host through generic `chain`/`cow-api` calls.

Per ADR-0007, the protocol logic itself lives in the `cowprotocol` crate, not in `nexum-engine`. This ADR consequently scopes the engine-side helpers to the WIT surface and the glue that wires the upstream primitives into the host call boundary.

The newer ComposableCoW iteration in development simplifies polling versus the watch-tower TypeScript implementation: less of the SDK's discriminated `PollResultCode` mapping may need to be replicated in `cowprotocol::composable` for `twap.poll-and-submit` to work. The rich `PollOutcome` variants described below remain the target API surface; the upstream implementation may end up simpler than the watch-tower mirror suggests.

## Decision

Add two new interfaces to package `shepherd:cow@0.2.0`:

```wit
interface twap {
    use nexum:host/types@0.2.0.{chain-id, log, host-error};
    use cow-api.{order-uid};

    /// Discriminated outcome of a single poll attempt against
    /// ComposableCoW. Mirrors watchtower's PollResultCode so modules
    /// avoid spamming RPC/orderbook when an order is known-not-ready.
    variant poll-outcome {
        submitted(order-uid),
        try-at-epoch(u64),      // unix seconds; module skips polls until then
        try-on-block(u64),      // specific block number
        try-next-block,         // default retry
        dont-try-again,         // terminal: TWAP completed or cancelled
    }

    poll-and-submit: func(
        chain-id: chain-id,
        registration: log,
    ) -> result<poll-outcome, host-error>;
}

interface ethflow {
    use nexum:host/types@0.2.0.{chain-id, log, host-error};
    use cow-api.{order-uid};

    submit-from-log: func(
        chain-id: chain-id,
        placement: log,
    ) -> result<order-uid, host-error>;
}
```

Both interfaces ship in the existing `shepherd` world alongside `cow-api`. `order-uid` is added to `cow-api` as `type order-uid = list<u8>` (56 bytes, validated host-side) and reused by all three interfaces; `cow-api/submit-order` keeps returning it instead of `string`. Capability names `"twap"` and `"ethflow"` are appended to `KNOWN_CAPABILITIES` so manifests can declare them under `[capabilities].required`.

Host implementations are thin wrappers (~20–30 LOC each) over three upstream primitives that land in `cowprotocol` first (see ADR-0007):

- `cowprotocol::composable::poll_and_build_order(provider, owner, params, proof) -> PollOutcome` — returns the same discriminated outcome (`Submitted`, `TryAtEpoch`, `TryOnBlock`, `TryNextBlock`, `DontTryAgain`). Backs `twap.poll-and-submit`.
- `cowprotocol::eth_flow::decode_placement(log)` — decodes `OrderPlacement` into `(owner, OrderCreation, OrderUid)` with the EIP-1271 signing scheme pointing at the EthFlow contract. Backs `ethflow.submit-from-log`.
- `cowprotocol::OrderPostError` (rich variants + `retry_hint()`) — typed orderbook submission errors with backoff/drop classification. Modules consume the hints to react to transient vs permanent failures without spamming.

The engine wires these primitives into HostState and maps their errors to `host-error` kinds; no protocol logic lives in `nexum-engine`. Modules continue to declare their own log subscriptions via `[[subscription]]` in `nexum.toml`; the helpers only decode and submit, they do not auto-subscribe.

## Considered options

- **Low-level primitives only** (`chain.eth-call`, `chain.keccak256`, `chain.sign-digest`, raw `cow-api/submit-order`). Maximally orthogonal, but every guest module re-derives the same EIP-712 / GPv2 / ComposableCoW / EthFlow glue. "Reuse over reimplement" applies: that derivation already lives in `cowprotocol::{Order, OrderBookApi, eth_flow, composable}` and should not be re-shipped in every WASM artifact.
- **Implement the protocol glue inside `nexum-engine` host code, port upstream later.** Rejected per ADR-0007: every line of TWAP polling or EthFlow decoding that lives in the engine is a line that future Rust consumers cannot reuse, and a line that diverges as cow-rs evolves.
- **EthFlow as pure passive observer (no `submit-from-log`).** Briefly considered after reading "watcher" / "monitor" in docs/00 and docs/04 as "no submission". Rejected after verifying that CoW's own autopilot DOES post equivalent (insert into `ethflow_orders` table); the Shepherd module is intended to externalize that role, not replace it with passive observation. The `pending_orders` state mentioned in docs/04 is a side-effect of the relay (local accounting of what's been observed), not the goal.
- **Simple `option<order-uid>` return on twap instead of `poll-outcome` variant.** A 1-hour-spaced TWAP polled every block would spam ~300 RPC calls per part with `None` returns. The richer outcome (`try-at-epoch`, etc.) matches watchtower's existing `PollResult` and lets modules skip polls until the contract says it's worth retrying. Production-critical.
- **Single combined interface** `shepherd:cow/orders` with both helpers. Cheaper world surface but harder to gate per-capability — a module that only watches EthFlow shouldn't have to import TWAP and vice versa. Splitting keeps `[capabilities].required` honest.
- **`log-json: list<u8>` payload** instead of the typed `nexum:host/types.log` record. The record already exists and the engine's event dispatch already projects `alloy_rpc_types_eth::Log` into it, so reuse wins on both ergonomics and "no duplicate decoders".
- **TWAP merkle-proof / `setRoot` support in v1.** Deferred. The 0.2 helper only handles `ComposableCoW.create()` (empty proof, single conditional order). `setRoot` polling requires off-chain proof derivation that itself warrants a separate helper (`twap.poll-and-submit-with-proof`) once a module actually needs it.
- **Bumping the package to `shepherd:cow@0.3.0`.** Not needed: adding imports to an existing world is additive under WIT subsumption rules. Modules compiled against the current 0.2.0 surface continue to build.

## Consequences

- `cow-api/submit-order` return type changes from `string` to `order-uid`. No external consumers today (0.2 is unreleased), so this is internal.
- Host helpers require a chain to be configured in `[chains.<id>]` — uncovered chains return `host-error.unsupported`. Same posture as `cow-api`.
- Orderbook idempotency (same UID on duplicate submit) is preserved but invisible to the module. Modules that need dedup must record UIDs in `local-store` themselves.
- TWAP modules must implement the `poll-outcome` state machine: persist `next_attempt` hints (epoch or block number) in local-store, skip polls until trigger, remove watches on `dont-try-again`. Without this, the poll loop becomes O(blocks × twaps) with most calls wasted. The M3 SDK is expected to ship a helper that encapsulates the state machine.
- Orderbook errors return as `host-error` with the original CoW error code in `code`. Modules use `OrderPostError::try_from(host_error)` plus `retry_hint()` (ADR-0007 item 3) to map to next-block / backoff / drop. Without this layered approach, modules spam the orderbook with permanently-broken orders.
- Implementation order: the three `cowprotocol` primitives (`composable::poll_and_build_order` with rich `PollOutcome`, `eth_flow::decode_placement`, `OrderPostError` rich + `retry_hint`) land in `bleu/cow-rs` first; `nullis-shepherd` adopts via the existing `[patch.crates-io]` rev bump (ADR-0004). Host-side issues stay blocked on upstream merges.
- Failure modes map onto existing `host-error-kind` variants (`invalid-input`, `denied`, `rate-limited`, `timeout`, `unavailable`, `unsupported`, `internal`). No new error taxonomy.

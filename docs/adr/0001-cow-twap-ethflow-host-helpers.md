---
status: proposed
---

# TWAP and EthFlow as intent helpers in `shepherd:cow@0.2.0`

## Context

The reference engine already exposes `shepherd:cow/cow-api` for raw orderbook access (REST passthrough + `submit-order`). Two further CoW workflows show up in every non-trivial module: ComposableCoW conditional orders (TWAP being the canonical example) and EthFlow native-ETH orders. Both share a tight pattern — observe an on-chain event, derive a signed `OrderCreation`, submit it to the orderbook — but the derivation has enough protocol detail (digest, signature scheme, app-data resolution, `getTradeableOrderWithSignature` eth_call against ComposableCoW) that a guest module would either ship that logic itself (large WASM, duplicates work in the `cowprotocol` Rust SDK) or make ten round-trips to the host through generic `chain`/`cow-api` calls.

The protocol logic itself — TWAP polling, EthFlow log decoding, app-data resolution — is not engine-specific. Every Rust consumer of CoW Protocol (indexers, bots, this engine) needs the same primitives. Per ADR-0007, those primitives belong in the `cowprotocol` crate, not in `nexum-engine`. This ADR consequently scopes the engine-side helpers to the WIT surface and the glue that wires the upstream primitives into the host call boundary.

## Decision

Add two new interfaces to package `shepherd:cow@0.2.0`:

```wit
interface twap {
    use nexum:host/types@0.2.0.{chain-id, log, host-error};
    use cow-api.{order-uid};
    poll-and-submit: func(
        chain-id: chain-id,
        registration: log,
    ) -> result<option<order-uid>, host-error>;
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

- `cowprotocol::composable::poll_and_build_order(provider, owner, params, proof)` — returns `Ready(OrderCreation, signature)` or `NotReady` on contract revert. Backs `twap.poll-and-submit`.
- `cowprotocol::eth_flow::decode_placement(log)` — returns `(owner, OrderCreation, OrderUid)` from an `OrderPlacement` event log. Backs `ethflow.submit-from-log`.
- `cowprotocol::app_data::OrderBookAppDataResolver` — given a chain id and a `bytes32` hash, returns the JSON document (with `EMPTY_APP_DATA_HASH` fast-path and LRU cache built in). Used by both helpers and any future module-facing path.

The engine wires these primitives into HostState and maps their errors to `host-error` kinds; no protocol logic lives in `nexum-engine`. Modules continue to declare their own log subscriptions via `[[subscription]]` in `nexum.toml`; the helpers only decode and submit, they do not auto-subscribe.

## Considered options

- **Low-level primitives only** (`chain.eth-call`, `chain.keccak256`, `chain.sign-digest`, raw `cow-api/submit-order`). Maximally orthogonal, but every guest module re-derives the same EIP-712 / GPv2 / ComposableCoW glue. mfw78's "reuse over reimplement" applied: that derivation already lives in `cowprotocol::{Order, OrderBookApi, eth_flow, composable}` and should not be re-shipped in every WASM artifact.
- **Implement the protocol glue inside `nexum-engine` host code, port upstream later.** Rejected per ADR-0007: every line of TWAP polling or EthFlow decoding that lives in the engine is a line that future Rust consumers cannot reuse, and a line that diverges as cow-rs evolves.
- **Single combined interface** `shepherd:cow/orders` with both helpers. Cheaper world surface but harder to gate per-capability — a module that only watches EthFlow shouldn't have to import TWAP and vice versa. Splitting keeps `[capabilities].required` honest.
- **Symmetric `result<option<order-uid>, host-error>` for both.** TWAP and EthFlow are genuinely asymmetric: TWAP is poll-driven and a `None` ("not tradeable yet") is the normal steady-state; EthFlow is event-driven and every accepted log produces exactly one UID. Forcing symmetry obscures semantics for callers.
- **`log-json: list<u8>` payload** instead of the typed `nexum:host/types.log` record. The record already exists and the engine's event dispatch already projects `alloy_rpc_types_eth::Log` into it, so reuse wins on both ergonomics and "no duplicate decoders".
- **TWAP merkle-proof / `setRoot` support in v1.** Deferred. The 0.2 helper only handles `ComposableCoW.create()` (empty proof, single conditional order). `setRoot` polling requires off-chain proof derivation that itself warrants a separate helper (`twap.poll-and-submit-with-proof`) once a module actually needs it.
- **Bumping the package to `shepherd:cow@0.3.0`.** Not needed: adding imports to an existing world is additive under WIT subsumption rules. Modules compiled against the current 0.2.0 surface continue to build.

## Consequences

- `cow-api/submit-order` return type changes from `string` to `order-uid`. No external consumers today (0.2 is unreleased), so this is internal.
- Host helpers require a chain to be configured in `[chains.<id>]` — uncovered chains return `host-error.unsupported`. Same posture as `cow-api`.
- Orderbook idempotency (same UID on duplicate submit) is preserved but invisible to the module. Modules that need dedup must record UIDs in `local-store` themselves.
- App-data resolution adds a GET to `api.cow.fi/{chain}/api/v1/app_data/{hash}` on the first sighting of a non-empty hash. The LRU cache and the GET itself live in `cowprotocol::app_data::OrderBookAppDataResolver` (ADR-0007 item 3); cache miss + orderbook miss surfaces as `host-error.unavailable`.
- Implementation order: the three `cowprotocol` primitives (`composable::poll_and_build_order`, `eth_flow::decode_placement`, `app_data::OrderBookAppDataResolver`) land in `bleu/cow-rs` first; `nullis-shepherd` adopts via the existing `[patch.crates-io]` rev bump (ADR-0002). Host-side issues stay blocked on upstream merges.
- Failure modes map onto existing `host-error-kind` variants (`invalid-input`, `denied`, `rate-limited`, `timeout`, `unavailable`, `unsupported`, `internal`). No new error taxonomy.

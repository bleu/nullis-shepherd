---
status: proposed
---

# Push CoW Protocol logic to `cow-rs` first, adopt in `nexum-engine` second

## Context

Implementing ADR-0006 (twap + ethflow host helpers) and ADR-0005 (cow-api backend) surfaces a recurring question: when the engine needs a piece of CoW Protocol logic that the `cowprotocol` Rust SDK does not yet expose (TWAP polling glue, EthFlow log decoding, rich orderbook error variants, custom orderbook URLs), do we write that logic locally in `nexum-engine` and tidy it up upstream later, or do we open the cow-rs PR first and only land the engine wiring afterwards?

mfw78's review of cow-rs PR #5 named the failure mode explicitly: duplicating work that an existing crate could do is the AI-coding anti-pattern most likely to land in a Bleu PR. The same risk applies to any engine-side reimplementation of protocol logic.

CoW's broader architecture has been moving the same direction: `watch-tower` extracted from `cowprotocol/services` autopilot, the `refunder` crate likewise, with the `ethflow_events` indexer (`crates/autopilot/src/database/onchain_order_events/ethflow_events.rs`) identified as the next extraction target. The Rust-side equivalent of those extractions is the right home for protocol primitives — `bleu/cow-rs` (then upstream into `cowdao-grants/cow-rs`), not the engine.

## Decision

Protocol-level CoW logic — anything that an indexer, a bot, or a non-`nexum` Rust consumer of CoW Protocol would also need — lands in `bleu/cow-rs` first as an upstream PR, and is consumed by `nexum-engine` via the existing `[patch.crates-io]` rev bump (ADR-0004). The engine never writes throwaway local copies of the same logic with the intent to "port later".

The concrete set of primitives we know we need is, in priority order:

1. **`cowprotocol::composable::poll_and_build_order(provider, owner, params, proof) -> Result<PollOutcome, _>`** — eth_call against `ComposableCoW.getTradeableOrderWithSignature`, decode return, rebuild `OrderCreation`. `PollOutcome` mirrors watchtower's `PollResultCode` (TS): `Submitted(OrderCreation, Vec<u8>)`, `TryAtEpoch(u64)`, `TryOnBlock(u64)`, `TryNextBlock`, `DontTryAgain`. Backs `twap.poll-and-submit` (ADR-0006).

2. **`cowprotocol::eth_flow::decode_placement(log) -> Result<DecodedPlacement, _>`** — decode `OrderPlacement` event log, reconstruct `OrderCreation` with the EIP-1271 signing scheme pointing at the `CoWSwapEthFlow` contract, compute `OrderUid`. Replicates the indexing logic currently inside `cowprotocol/services/crates/autopilot/src/database/onchain_order_events/ethflow_events.rs`. Backs `ethflow.submit-from-log` (ADR-0006).

3. **`cowprotocol::OrderPostError` rich variants + `retry_hint(&self) -> RetryHint`** — typed orderbook submission errors (`QuoteNotFound`, `InvalidQuote`, `InsufficientAllowance`, `InsufficientBalance`, `TooManyLimitOrders`, `InvalidAppData`, `AppDataFromMismatch`, `SellAmountOverflow`, `ZeroAmount`, `TransferSimulationFailed`, `ExcessiveValidTo`, …) with a `retry_hint()` helper classifying each into `TryNextBlock`, `BackoffSeconds(u64)`, or `Drop`. Mirrors watchtower's `API_ERRORS_TRY_NEXT_BLOCK` / `API_ERRORS_BACKOFF` / `API_ERRORS_DROP` tables. Without this, every Rust consumer of CoW reinvents the same mapping, and modules spam the orderbook with permanently-broken orders. **Critical-path, not optional.**

4. **`cowprotocol::OrderBookApi::with_base_url(chain_id, base_url)`** — custom-URL constructor for barn / staging / forked deployments. Unblocks per-chain orderbook URL overrides in `engine.toml` (ADR-0005).

5. **`cowprotocol` `wasm32` compatibility** — feature-gate the `reqwest` dependency so guest modules can use the pure types (`Order`, `OrderCreation`, `OrderUid`, `composable::*`, `eth_flow::decode_*`) without dragging in an HTTP client. Unblocks M3 SDK guest modules consuming `cowprotocol` directly.

Lower-priority follow-ons (`OrderUid::from_slice`, retry middleware on `OrderBookApi`, `OrderCreation::from_gpv2`) are good-to-have but are not blocking for the M2 host scope.

## Considered options

- **Implement locally, refactor upstream later.** Faster short term but predictably leaves an indeterminate amount of duplicated logic in the engine, contradicts mfw78's stated conventions, and grows technical debt every time cow-rs evolves the underlying types. Rejected.
- **Wait for cow-rs upstream maintainers to add these on their own.** No evidence anyone else is doing this work; the grant timeline does not permit waiting.
- **Vendor a fork of cow-rs inside `nullislabs/shepherd`.** Worst of all worlds: blocks neither the engine nor cow-rs from drifting, and forces every other CoW consumer to re-derive the same primitives.
- **Simple `Ready/NotReady` PollOutcome on item 1.** Rejected: doesn't capture watchtower's `TRY_AT_EPOCH(t)` hint, which is what prevents the polling loop from RPC-spamming during the 1-hour gap between TWAP parts.

## Consequences

- Every M2 engine issue that consumes one of the five primitives above is blocked on its cow-rs PR merging. We sequence issues so that upstream PRs and engine adoption can land in parallel where possible (e.g., open items 1, 2, 3 against `bleu/cow-rs` simultaneously rather than serially).
- `[patch.crates-io]` rev in the workspace `Cargo.toml` (ADR-0004) is bumped after each cow-rs merge; the bump is the engine's signal that a new primitive is consumable.
- PRs in `bleu/cow-rs` follow the existing mfw78 conventions established by cow-rs PR #5: severity-tagged reviews, alloy reuse over local reimplementation, GPL-3.0, edition 2024, terse rustdoc.
- After acceptance in `bleu/cow-rs`, each primitive is also surfaced as a PR (or backport) against `cowdao-grants/cow-rs` so the wider ecosystem benefits and the bleu fork narrows over time.
- The engine repo stays small: `nexum-engine` contains WIT, host wiring, supervisor, redb store, alloy provider pool, and `engine.toml` schema — nothing about CoW Protocol semantics.
- The rich `PollOutcome` (item 1) + `OrderPostError` + `retry_hint` (item 3) design naturally leads to tighter M3 SDK helpers: `WatchSet`, `PollLoop`, `BackoffLedger` patterns that any module re-using `shepherd-sdk` gets for free.
- A follow-on Bleu module — the Rust-side equivalent of `cowprotocol/refunder` (permissionless `invalidateOrder` triggering for expired EthFlow orders) — becomes natural to ship once `ethflow.submit-from-log` lands. Out of scope for M2 but explicitly enabled by the same primitives.

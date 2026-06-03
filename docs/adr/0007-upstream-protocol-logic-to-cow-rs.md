---
status: proposed
---

# Push CoW Protocol logic to `cow-rs` first, adopt in `nexum-engine` second

## Context

Implementing ADR-0006 (twap + ethflow host helpers) and ADR-0005 (cow-api backend) surfaces a recurring question: when the engine needs a piece of CoW Protocol logic that the `cowprotocol` Rust SDK does not yet expose (TWAP polling glue, EthFlow log decoding, rich orderbook error variants, custom orderbook URLs), do we write that logic locally in `nexum-engine` and tidy it up upstream later, or do we add it to the open upstream PR first and only land the engine wiring afterwards?

Review feedback on cow-rs PR #5 named the failure mode explicitly: duplicating work that an existing crate could do is the AI-coding anti-pattern most likely to land in a contribution. The same risk applies to any engine-side reimplementation of protocol logic.

CoW maintainers have signalled intent to keep extracting services from the `cowprotocol/services` monolith: `watch-tower` is already extracted, the `refunder` crate likewise, and the `ethflow_events` indexer (`crates/autopilot/src/database/onchain_order_events/ethflow_events.rs`) is the next extraction target. The Rust SDK that Bleu is delivering through PR #5 is the natural home for the protocol primitives those extractions need.

## Decision

Protocol-level CoW logic, meaning anything that an indexer, a bot, or a non-`nexum` Rust consumer of CoW Protocol would also need, lands as additional commits on `cowdao-grants/cow-rs` PR #5 first (head branch `bleu/cow-rs:main`), and is consumed by `nexum-engine` via the `[patch.crates-io]` rev bump (ADR-0004). The engine never writes throwaway local copies of the same logic with the intent to "port later".

The concrete set of primitives we know we need is, in priority order:

1. **`cowprotocol::composable::poll_and_build_order(provider, owner, params, proof) -> Result<PollOutcome, _>`** — eth_call against `ComposableCoW.getTradeableOrderWithSignature`, decode return, rebuild `OrderCreation`. `PollOutcome` mirrors watchtower's `PollResultCode` (TS): `Submitted(OrderCreation, Vec<u8>)`, `TryAtEpoch(u64)`, `TryOnBlock(u64)`, `TryNextBlock`, `DontTryAgain`. Backs `twap.poll-and-submit` (ADR-0006).

2. **`cowprotocol::eth_flow::decode_placement(log) -> Result<DecodedPlacement, _>`** — decode `OrderPlacement` event log, reconstruct `OrderCreation` with the EIP-1271 signing scheme pointing at the `CoWSwapEthFlow` contract, compute `OrderUid`. Replicates the indexing logic currently inside `cowprotocol/services/crates/autopilot/src/database/onchain_order_events/ethflow_events.rs`. Backs `ethflow.submit-from-log` (ADR-0006).

3. **`cowprotocol::OrderPostError` rich variants + `retry_hint(&self) -> RetryHint`** — typed orderbook submission errors (`QuoteNotFound`, `InvalidQuote`, `InsufficientAllowance`, `InsufficientBalance`, `TooManyLimitOrders`, `InvalidAppData`, `AppDataFromMismatch`, `SellAmountOverflow`, `ZeroAmount`, `TransferSimulationFailed`, `ExcessiveValidTo`, …) with a `retry_hint()` helper classifying each into `TryNextBlock`, `BackoffSeconds(u64)`, or `Drop`. Mirrors watchtower's `API_ERRORS_TRY_NEXT_BLOCK` / `API_ERRORS_BACKOFF` / `API_ERRORS_DROP` tables. Without this, every Rust consumer of CoW reinvents the same mapping, and modules spam the orderbook with permanently-broken orders. **Critical-path, not optional.**

4. **`cowprotocol::OrderBookApi::with_base_url(chain_id, base_url)`** — custom-URL constructor for barn / staging / forked deployments. Unblocks per-chain orderbook URL overrides in `engine.toml` (ADR-0005).

5. **`cowprotocol` `wasm32` compatibility** — feature-gate the `reqwest` dependency so guest modules can use the pure types (`Order`, `OrderCreation`, `OrderUid`, `composable::*`, `eth_flow::decode_*`) without dragging in an HTTP client. Unblocks M3 SDK guest modules consuming `cowprotocol` directly.

Lower-priority follow-ons (`OrderUid::from_slice`, retry middleware on `OrderBookApi`, `OrderCreation::from_gpv2`) are good-to-have but are not blocking for the M2 host scope.

## Considered options

- **Implement locally, refactor upstream later.** Faster short term but predictably leaves an indeterminate amount of duplicated logic in the engine, contradicts the conventions established on cow-rs PR #5, and grows technical debt every time cow-rs evolves the underlying types. Rejected.
- **Wait for cow-rs upstream maintainers to add these on their own.** No evidence anyone else is doing this work; the grant timeline does not permit waiting.
- **Vendor a fork of cow-rs inside `nullislabs/shepherd`.** Worst of all worlds: blocks neither the engine nor cow-rs from drifting, and forces every other CoW consumer to re-derive the same primitives.
- **Simple `Ready/NotReady` PollOutcome on item 1.** Rejected: doesn't capture watchtower's `TRY_AT_EPOCH(t)` hint, which is what prevents the polling loop from RPC-spamming during the 1-hour gap between TWAP parts.

## Consequences

- Every M2 engine issue that consumes one of the five primitives above is blocked on the corresponding commit landing in PR #5's head branch. Items 1, 2, 3 can be authored as independent commits and pushed in parallel rather than serially.
- `[patch.crates-io]` rev in the workspace `Cargo.toml` (ADR-0004) is bumped after each push to PR #5; the bump is the engine's signal that a new primitive is consumable.
- Commits added to PR #5 follow the conventions established by its review thread: severity-tagged review notes, alloy reuse over local reimplementation, GPL-3.0, edition 2024, terse rustdoc.
- The engine repo stays small: `nexum-engine` contains WIT, host wiring, supervisor, redb store, alloy provider pool, and `engine.toml` schema, with nothing about CoW Protocol semantics.
- The rich `PollOutcome` (item 1) plus `OrderPostError` and `retry_hint` (item 3) design naturally leads to tighter M3 SDK helpers: `WatchSet`, `PollLoop`, `BackoffLedger` patterns that any module re-using `shepherd-sdk` gets for free.
- A follow-on Bleu module, the Rust-side equivalent of `cowprotocol/refunder` (permissionless `invalidateOrder` triggering for expired EthFlow orders), becomes natural to ship once `ethflow.submit-from-log` lands. Out of scope for M2 but explicitly enabled by the same primitives.

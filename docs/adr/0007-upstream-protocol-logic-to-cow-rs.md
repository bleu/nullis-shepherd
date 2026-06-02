---
status: proposed
---

# Push CoW Protocol logic to `cow-rs` first, adopt in `nexum-engine` second

## Context

Implementing ADR-0001 (twap + ethflow host helpers) and ADR-0004 (cow-api backend) surfaces a recurring question: when the engine needs a piece of CoW Protocol logic that the `cowprotocol` Rust SDK does not yet expose (TWAP polling glue, EthFlow log decoding, app-data hash-to-document resolution, custom orderbook URLs), do we write that logic locally in `nexum-engine` and tidy it up upstream later, or do we open the cow-rs PR first and only land the engine wiring afterwards?

mfw78's review of cow-rs PR #5 named the failure mode explicitly: duplicating work that an existing crate could do is the AI-coding anti-pattern most likely to land in a Bleu PR. The same risk applies to any engine-side reimplementation of protocol logic.

## Decision

Protocol-level CoW logic — anything that an indexer, a bot, or a non-`nexum` Rust consumer of CoW Protocol would also need — lands in `bleu/cow-rs` first as an upstream PR, and is consumed by `nexum-engine` via the existing `[patch.crates-io]` rev bump (ADR-0002). The engine never writes throwaway local copies of the same logic with the intent to "port later".

The concrete set of primitives we know we need is, in priority order:

1. `cowprotocol::composable::poll_and_build_order(provider, owner, params, proof) -> Result<PollOutcome, _>` — eth_call against `ComposableCoW.getTradeableOrderWithSignature`, decode return, rebuild `OrderCreation`. Backs `twap.poll-and-submit`.
2. `cowprotocol::eth_flow::decode_placement(log) -> Result<DecodedPlacement, _>` — decode `OrderPlacement` event log, reconstruct `OrderCreation` and `OrderUid`. Backs `ethflow.submit-from-log`.
3. `cowprotocol::app_data::OrderBookAppDataResolver` — `AppDataResolver` trait + cached implementation around `OrderBookApi::app_data(hash)`, with `EMPTY_APP_DATA_HASH` fast-path. Used by twap, ethflow, and any future caller that needs to materialise an app-data document.
4. `cowprotocol::OrderBookApi::with_base_url(chain_id, base_url)` — custom-URL constructor for barn / staging / forked deployments.
5. `cowprotocol` `wasm32` compatibility — feature-gate the `reqwest` dependency so guest modules can use the pure types (`Order`, `OrderCreation`, `OrderUid`, `composable::*`, `eth_flow::decode_*`) without dragging in an HTTP client.

Lower-priority follow-ons (richer `OrderBookApiError` variants, `OrderUid::from_slice`, retry middleware, `OrderCreation::from_gpv2`) are good-to-have but are not blocking for the M2 host scope.

## Considered options

- **Implement locally, refactor upstream later.** Faster short term but predictably leaves an indeterminate amount of duplicated logic in the engine, contradicts mfw78's stated conventions, and grows technical debt every time cow-rs evolves the underlying types. Rejected.
- **Wait for cow-rs upstream maintainers to add these on their own.** No evidence anyone else is doing this work; the grant timeline does not permit waiting.
- **Vendor a fork of cow-rs inside `nullislabs/shepherd`.** Worst of all worlds: blocks neither the engine nor cow-rs from drifting, and forces every other CoW consumer to re-derive the same primitives.

## Consequences

- Every M2 engine issue that consumes one of the five primitives above is blocked on its cow-rs PR merging. We sequence issues so that upstream PRs and engine adoption can land in parallel where possible (e.g., open all three protocol-helper PRs against `bleu/cow-rs` simultaneously rather than serially).
- `[patch.crates-io]` rev in the workspace `Cargo.toml` (ADR-0002) is bumped after each cow-rs merge; the bump is the engine's signal that a new primitive is consumable.
- PRs in `bleu/cow-rs` follow the existing mfw78 conventions established by cow-rs PR #5: severity-tagged reviews, alloy reuse over local reimplementation, GPL-3.0, edition 2024, terse rustdoc.
- After acceptance in `bleu/cow-rs`, each primitive is also surfaced as a PR (or backport) against `cowdao-grants/cow-rs` so the wider ecosystem benefits and the bleu fork narrows over time.
- The engine repo stays small: `nexum-engine` contains WIT, host wiring, supervisor, redb store, alloy provider pool, and `engine.toml` schema — nothing about CoW Protocol semantics.

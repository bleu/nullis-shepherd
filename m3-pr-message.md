# M3 epic: SDK + examples + tutorial + QA validation

## Summary

Builds on M2 (`dev/m2`). M3 ships the **layered SDK + developer experience** that lets a module author write strategy logic against `&impl Host` (testable without wasm) while the production wit-bindgen adapter ships as mechanical glue.

## Core deliverable

| Crate / module | What it adds |
|---|---|
| `crates/shepherd-sdk` | 4 per-capability host traits (`ChainHost`, `LocalStoreHost`, `CowApiHost`, `LoggingHost`) + supertrait `Host`; SDK-side `HostError` mirroring the wit struct; helpers in `chain` (`eth_call_params`, `parse_eth_call_result`, `decode_revert_hex`) and `cow` (`PollOutcome`, `RetryAction`, `classify_api_error`, `gpv2_to_order_data`, `decode_revert`, `IConditionalOrder` sol! interface). |
| `crates/shepherd-sdk-test` | `MockHost` with per-trait mocks (`MockChain`, `MockLocalStore`, `MockCowApi`, `MockLogging`) — enables module unit tests that run as native Rust, no wasm toolchain. |
| `modules/examples/price-alert` | Chainlink oracle reader. Demonstrates `chain::request` + ABI decode + threshold logic. |
| `modules/examples/balance-tracker` | ERC-20 balance differ. Demonstrates raw `chain::request` + per-key `local-store` persistence. |
| `modules/examples/stop-loss` | Full M3 surface: oracle read + `OrderCreation` with `Signature::PreSign` + cow-api submit + typed retry classification. |
| `docs/tutorial-first-module.md` | Reads as a guided tour of the real stop-loss module instead of inlined snippets with `todo!()`. |
| Strategy / lib.rs split | M2 modules (twap-monitor, ethflow-watcher) refactored to consume the Host trait pattern + SDK helpers (ADR-0009). |

## Architectural review request

This is **the surface flagged for explicit review**: the host module architecture.

[ADR-0009](docs/adr/0009-host-trait-surface.md) captures three coupled decisions:

1. **Four per-capability traits + supertrait `Host`** with blanket impl. Lets strategy code be `<H: Host>` generic; tests inject `MockHost`, production injects `WitBindgenHost`.
2. **SDK-side `HostError` mirroring the wit struct field-for-field**, bridged via per-module `From` impls. Keeps `shepherd-sdk-test` world-neutral so mocks compile without a wasm toolchain.
3. **Per-module `strategy.rs` + `lib.rs` split**: strategy is pure logic; lib.rs is the wit-bindgen + WitBindgenHost adapter + Guest impl.

## Bugs surfaced + fixed during testnet wiring

Two M1-tail bugs the M3 testnet runbook exposed (live on Sepolia, both fixed in this epic):

1. **`runtime/event_loop.rs`**: `select_all` over an empty `Vec` yielded `None` immediately, tripping the "stream ended -> shut down" arm before any event flowed. Fix: substitute `stream::pending()` when the Vec is empty. Regression test added.
2. **`Supervisor::load`**: init-failed modules stayed `alive = true` and received every block dispatch, wasting fuel on a no-op. Fix: flip `alive = false` when `init` returns `Err`. Regression test added.

## Validation

- **Unit tests**: 181 tests passing (including 6 doctests).
- **Supervisor integration tests**: 5 module-specific + 2 regression tests.
- **Live testnet (Sepolia)**: `docs/operations/m3-testnet-runbook.md` walks 3 modules end-to-end. `docs/operations/m3-edge-case-validation.md` runs 5 error-path scenarios, all passing.
- `cargo clippy --all-targets --workspace -- -D warnings` clean.
- `cargo fmt --all --check` clean.
- `cargo doc --workspace --no-deps -D warnings` clean (CI gate added).
- WASM builds for all 5 modules under `wasm32-wasip2 --release` (CI matrix).

Closes BLEU-825 through BLEU-855, COW-1063, COW-1066 through COW-1070.

Linear milestone: [M3 - SDK + Developer Experience](https://linear.app/bleu-builders/project/shepherd). Companion: M2 (`dev/m2`).

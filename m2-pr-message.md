# M2 epic: TWAP + EthFlow modules + module.toml manifests

## Summary

Builds on M1 (`dev/m1`). M2 ships **two production-shaped modules** that consume the M1 host surface end-to-end, plus engine hardening and additional test coverage.

### Modules

- **`modules/twap-monitor/`** — indexes `ComposableCoW.ConditionalOrderCreated` events into local-store, polls watches via `eth_call` against `getTradeableOrderWithSignature`, decodes `PollOutcome` from the return/revert data, builds `OrderCreation` with EIP-1271 signatures, submits via cow-api, and applies `OrderPostError::retry_hint()` for typed retry classification (`TryNextBlock` / `TryAtEpoch` / `DontTryAgain` / `Drop`).
- **`modules/ethflow-watcher/`** — decodes `CoWSwapEthFlow.OrderPlacement` logs, lifts the embedded `GPv2OrderData` into an `OrderCreation` with `Signature::Eip1271` (or `PreSign` for non-EthFlow chains), submits via cow-api, applies the same retry classification, and persists `submitted:{uid}` / `dropped:{uid}` / `backoff:{uid}` keys for re-delivery idempotency.

Both modules ship with `module.toml` manifests declaring capability requirements and subscription filters.

### Engine hardening (on top of M1)

- `strum::IntoStaticStr` derived on all error enums for structured-log metric labels
- Rustdoc intra-doc link fixes after the `pub(crate)` visibility sweep
- Rust-idiomatic compliance pass (em-dash cleanup, `#[from]` on error variants, unused dep silencing)
- `local_store_err` helper extracted to centralise `StorageError` -> `HostError` mapping
- `tracing::warn!` for nexum.toml deprecation (was `eprintln!`)
- `cowprotocol` patch bumped to `bleu/cow-rs` rev `57f5f55` (BLEU-822 `OrderPostErrorKind` + `retry_hint()`, BLEU-823 `OrderBookApi::with_base_url`)

### Test coverage

- **twap-monitor**: 34 unit tests — ABI decoder round-trips, revert selector dispatch, `PollOutcome` lifecycle, `OrderCreation` builder, retry classification, watch-key parsing
- **ethflow-watcher**: 10 unit tests — placement decoder, `OrderCreation` builder (EIP-1271 + PreSign), retry classification, unsupported-chain rejection, non-empty app_data rejection
- **nexum-engine**: 54 tests (12 new) — cow_orderbook error paths, provider_pool edge cases, local_store_redb concurrent access

Total: **98 tests passing**.

### cow-rs dependency

Patches `cowprotocol` to `bleu/cow-rs` main (rev `57f5f55`). The fork carries:

- BLEU-822 `OrderPostErrorKind` + `retry_hint()` on `ApiError`
- BLEU-823 `OrderBookApi::with_base_url(chain, base_url)`

Drop the patch once `cowprotocol >= 1.0.0-alpha.4` ships upstream. Tracked as ADR-0007 + ADR-0004.

### Validation

```
cargo fmt --all --check                                    # clean
RUSTFLAGS=-D warnings cargo clippy --all-targets --workspace  # clean
cargo test -p nexum-engine -p twap-monitor -p ethflow-watcher  # 98 passed
```

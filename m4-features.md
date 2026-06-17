# M4 Features & Changes (dev/m4-base vs dev/m3-rebase)

Diff summary: 82 files changed, 8305 insertions(+), 545 deletions(-)

## 1. Supervisor: Restart & Poison Policies (COW-1032, COW-1033)

### Exponential-backoff restart (COW-1033)
- New file: `crates/nexum-engine/src/runtime/restart_policy.rs`
- `backoff_for(failure_count)` — 1s, 2s, 4s, ... capped at 5 min
- Supervisor re-instantiates the WASM component (fresh `Store` + bindings) on restart
- Failure counter resets on successful `on_event`

### Poison-pill detection & quarantine (COW-1032)
- New file: `crates/nexum-engine/src/runtime/poison_policy.rs`
- `PoisonPolicy { max_failures: 5, window: 10min }` — quarantines modules that trap repeatedly
- Poisoned modules are permanently removed from dispatch (no further restarts)
- `shepherd_module_poisoned{module}` gauge metric emitted

### Supervisor changes (`crates/nexum-engine/src/supervisor.rs`)
- `LoadedModule` now tracks `failure_count`, `next_attempt`, `failure_timestamps`, `poisoned`
- `dispatch_block` / `dispatch_log` respect restart backoff and poison state
- Module reinstantiation on restart (fresh Store, re-call `init`)
- Init failure treated as permanent (no restart) — COW-1070
- Multi-chain isolation (COW-1073): per-module restart/poison/fuel are independent across chains

### Supervisor tests (`crates/nexum-engine/src/supervisor/tests.rs`)
- Multi-chain isolation regression tests (COW-1073)
- Supervisor integration tests for 5 production modules (COW-1068)

## 2. Event Loop: WS Reconnect & Graceful Shutdown (COW-1071, COW-1072)

### WS reconnect with exponential backoff (COW-1071)
- `crates/nexum-engine/src/runtime/event_loop.rs` — major rewrite
- Per-stream reconnect tasks (`reconnecting_block_task`, `reconnecting_log_task`)
- `HEALTHY_WINDOW` (60s) before backoff counter resets
- `StreamError` typed error replaces `anyhow::Error` in stream items
- `shepherd_stream_reconnects_total` counter metric

### Graceful shutdown (COW-1072)
- Event dispatch separated into NextEvent enum (Block/Log/Shutdown/StreamPanic)
- Dispatch happens outside `tokio::select!` — no mid-dispatch cancellation
- `tasks.shutdown().await` drains reconnect tasks on shutdown
- Logs dispatched_blocks, dispatched_logs, uptime_secs on exit

### Last-block persistence (COW-1072)
- Part of `feat(event-loop+supervisor): graceful shutdown + last-block persistence`

## 3. Prometheus Metrics (COW-1034)

- `feat(metrics): Prometheus /metrics endpoint + 4 recording sites`
- `/metrics` endpoint configurable via `[engine.metrics]` in engine.toml
- 4 recording sites across the engine
- `engine.e2e.toml` and `engine.load.toml` both configure `bind_addr = "127.0.0.1:9100"`

## 4. JSON Logging (COW-1035)

- `feat(logging): JSON formatter + structured dispatch fields`
- Default output is now JSON; `--pretty-logs` CLI flag keeps human-readable format
- justfile `run-m2` and `run-m3` updated to pass `--pretty-logs`

## 5. SDK: `#[non_exhaustive]` on Host Enums (COW-1029)

- `HostErrorKind` and `LogLevel` marked `#[non_exhaustive]`
- Module adapters (`sdk_err_into_wit`, `convert_level`) now carry wildcard arms
- `wit_bindgen_macro.rs` updated with wildcard mappings

## 6. SDK: `cow_api_request` + `resolve_app_data` (COW-1074, COW-1075)

### New `CowApiHost::cow_api_request` method
- `crates/shepherd-sdk/src/host.rs` — new trait method for generic REST requests against orderbook
- `crates/shepherd-sdk/src/wit_bindgen_macro.rs` — wires to `shepherd::cow::cow_api::request`
- `crates/shepherd-sdk-test/src/lib.rs` — `MockCowApi` extended with `request_responses`, `respond_to_request_for`, `respond_to_request`

### New `resolve_app_data` helper (COW-1074)
- New file: `crates/shepherd-sdk/src/cow/app_data.rs`
- Resolves 32-byte `appData` hash to JSON via `GET /api/v1/app_data/{hex}`
- Short-circuits for `EMPTY_APP_DATA_HASH` without host call
- Uses `B256` instead of `[u8; 32]` across the surface

### SDK: `[u8; 32]` → `B256` refactor
- `refactor(sdk): replace [u8; 32] with B256 across resolve_app_data surface`

### cow-api: forward ApiError envelope (COW-1075)
- `fix(cow-api): forward orderbook ApiError envelope to HostError.data`
- Engine-side `cow_orderbook.rs` changes to forward error details

### cow-orderbook: extract DEFAULT_CHAINS const
- `refactor(cow-orderbook): extract DEFAULT_CHAINS const`

### Chainlink StubHost fix
- `fix(shepherd-sdk): add cow_api_request to chainlink StubHost + appData doc link`

## 7. Module Strategy Updates

### twap-monitor
- `resolve_app_data` integrated into submit path (COW-1074)
- Uses SDK helpers: `RetryAction`, `classify_api_error`, `gpv2_to_order_data`, `try_decode_api_error`
- Hex helpers via `alloy_primitives::hex::encode`

### ethflow-watcher
- `resolve_app_data` applied to `submit_placement` (COW-1074)
- `ExcessiveValidTo` drops downgraded to `Info` (COW-1076)
- Uses SDK cow helpers

### stop-loss (example)
- Extended strategy (`modules/examples/stop-loss/src/strategy.rs` +34 lines)
- `module.toml` updated

## 8. Test Fixture Modules (COW-1036)

- New `modules/fixtures/flaky-bomb/` — flaky module for poison-pill testing
- New `modules/fixtures/fuel-bomb/` — exhausts fuel budget for trap-isolation testing
- New `modules/fixtures/memory-bomb/` — exhausts memory for trap-isolation testing
- `test(resource-limits): 2 evil fixtures + 3 trap-isolation tests`

## 9. E2E Testnet Integration (COW-1064)

### Engine config
- New `engine.e2e.toml` — boots all 5 modules on Sepolia

### Scripts (`scripts/`)
- New `scripts/e2e-run.sh` — boots engine, captures metrics baseline
- New `scripts/e2e-onchain.sh` — submits TWAP + EthFlow on-chain
- New `scripts/e2e-finish.sh` — SIGINTs engine, generates report
- New `scripts/e2e-report-gen.sh` — auto-fills COW-1064 report template
- New `scripts/lib.sh` — shared shell functions
- New `scripts/env-template` — .env template for secrets
- New `scripts/_ethflow_quote.py` — EthFlow quote helper
- New `scripts/_twap_calldata.py` — TWAP calldata derivation
- New `scripts/README.md` — documentation for scripts
- Multiple script fixes: macOS bash 3.2 compat, JSON-shape log matching, WETH sentinel, idempotent submission, flat JSON report-gen, TWAP calldata t0=now-60, REPORTS_DIR ordering

### Documentation
- New `docs/operations/e2e-testnet-runbook.md`
- New `docs/operations/e2e-cow-1064-prep.md`
- New `docs/operations/e2e-reports/e2e-report-2026-06-18.md` (run report)
- New `docs/operations/e2e-reports/e2e-report.template.md`

### justfile
- New `build-e2e` and `run-e2e` targets

## 10. Load Testing (COW-1079, COW-1080)

### Tools
- New `tools/load-gen/` — Rust CLI: `--parallel` mode + aggressive saturation report
- New `tools/orderbook-mock/` — Rust mock orderbook server

### Engine config
- New `engine.load.toml` — Anvil fork + mock orderbook config

### Scripts
- New `scripts/load-bootstrap.sh` — starts Anvil + orderbook-mock
- New `scripts/load-run.sh` — submits N TWAP + M EthFlow per block
- New `scripts/load-teardown.sh`

### Reports
- New `docs/operations/load-reports/load-5x5-2026-06-19.md`
- New `docs/operations/load-reports/load-20x20-2026-06-19.md`
- New `docs/operations/load-reports/load-50x50-2026-06-19.md`
- New `docs/operations/load-reports/load-50x50-parallel-2026-06-19.md`

### Documentation
- New `docs/operations/load-testnet-runbook.md`

### Fixes
- `fix(load-gen): explicit nonce + unique EthFlow sellAmount (COW-1080)`

## 11. Production Deployment Guide (COW-1030)

- New `docs/production.md` (703 lines)

## 12. CI Improvements

- `ci: build all production module .wasm targets via matrix (COW-1066)`
- `ci: gate cargo doc warnings (-D warnings) + fix 3 broken intra-doc links (COW-1069)`
- Checkout action pinned to v6.0.2 (downgrade from v6.0.3)

## 13. Rust Idiomatic Compliance

- `chore(rust-idiomatic): M4 compliance pass (blockers + majors) (#66)`
- `chore(rust-idiomatic): M2 compliance pass (filtered from M4/M5 compliance)`
- Various strum::IntoStaticStr derives, em-dash cleanup, cargo fmt sweep

## 14. Engine Config Changes

- `engine.example.toml`: removed `[limits]` table (limits now per-module in supervisor)
- `engine.e2e.toml`: new E2E config with metrics enabled
- `engine.load.toml`: new load test config with Anvil fork
- `engine_config.rs`: changes to config parsing (~79 lines changed)

## 15. Dependency Version Changes

- alloy-primitives: 1.6 → 1.5
- alloy-sol-types: 1.6 → 1.5
- alloy-provider: 1.8 → 1.5
- alloy-rpc-types-eth: 1.8 → 1.5
- alloy-transport-ws: 1.8 → 1.5
- wit-bindgen: 0.58 → 0.57

## 16. Other Engine Changes

### Host backends
- `cow_api.rs`: +117 lines — cow_api_request impl, ApiError forwarding
- `local_store_redb.rs`: refactored (~87 lines changed)
- `provider_pool.rs`: changes (~63 lines changed)
- `host/state.rs`: minor changes

### Manifest
- `manifest/load.rs`: changes to loading logic
- `manifest/error.rs`: minor changes

### CLI
- `cli.rs`: `--pretty-logs` flag addition
- `main.rs`: restructured (~75 lines changed)

### Misc docs
- ADR-0009 updated (non_exhaustive applied note)
- QA findings resolved
- Various rustdoc fixes

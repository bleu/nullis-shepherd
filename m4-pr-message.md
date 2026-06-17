# M4 epic: production hardening + E2E + load testing

## Summary

Builds on M3 (`dev/m3`). M4 takes the SDK + modules from M3 and hardens the runtime around them so a single Shepherd instance is operable as a production daemon: bounded resource use, supervised crash recovery, structured observability, and an end-to-end testnet harness.

## Core deliverable

| Area | What landed |
|---|---|
| Resource limits + non-exhaustive SDK enums | `crates/nexum-engine` enforces per-module fuel + memory budgets; SDK error enums made `#[non_exhaustive]` so SDK bumps don't silently drop arms in module code (COW-1029, COW-1036). |
| Auto-restart + graceful shutdown + poison-pill | `Supervisor` restarts crashed module instances behind a backoff; SIGTERM/SIGINT drain in-flight work; a module that crashes N times in a row is parked rather than restart-spammed (COW-1033, COW-1072, COW-1032). |
| WS reconnect + structured logging + Prometheus metrics | `runtime/event_loop.rs` rebuilds WS subscriptions after disconnect; `tracing` + JSON output across the engine; `/metrics` scrape endpoint with per-module counters (COW-1071, COW-1035, COW-1034). |
| Multi-chain isolation | Per-chain hosts + per-chain local-store namespacing; one chain's RPC failure does not bleed into another (COW-1073). |
| E2E testnet integration + deployment guide | `docs/operations/e2e-testnet-runbook.md` walks a full Sepolia round-trip; `docs/production.md` covers operator setup, env vars, log shipping, scrape config (COW-1064, COW-1030). |
| AppData resolver via orderbook | `shepherd-sdk::cow` resolves app-data digests through the orderbook resolver endpoint, removing the IPFS hard dependency on submit paths (COW-1074). |
| Orderbook error envelope forwarded | `HostError.data` now carries the orderbook's structured error envelope so module code can decode `OrderPostErrorKind` without re-parsing JSON (COW-1075). |
| EthFlow ExcessiveValidTo + TWAP calldata helper | `modules/ethflow-watcher` downgrades the known-benign `ExcessiveValidTo` drop to `Info`; `scripts/_twap_calldata.py` produces a fresh-`t0` TWAP fixture for the e2e harness (COW-1076, COW-1077). |
| Load-test harness + load-gen calibration | `tools/load-gen` + `tools/orderbook-mock` + `scripts/load-run.sh` drive baseline / medium / saturation runs against an Anvil fork; reports under `docs/operations/load-reports/` (COW-1079, COW-1080). |

## Validation

- `cargo fmt --all -- --check` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo test --workspace` — **214 tests passing**; load-test harness has its own `tools/` test pass.
- WASM matrix (`wasm32-wasip2 --release`) green for all modules in CI.
- Live Sepolia smoke: e2e-testnet runbook walked end-to-end; load-reports under `docs/operations/load-reports/` document engine behaviour at 5x5 / 20x20 / 50x50 grids.
- Rustdoc `-D warnings` CI gate still clean.

## M4-specific paths for review

- `crates/nexum-engine/src/{runtime,supervisor,host}/**` (resource limits, supervisor restart, WS reconnect, multi-chain isolation, error envelope forwarding)
- `crates/nexum-engine/src/engine_config.rs` (Prometheus + log config)
- `modules/ethflow-watcher/src/strategy.rs` (`ExcessiveValidTo` downgrade)
- `modules/twap-monitor/src/strategy.rs` (AppData resolver consumption)
- `crates/shepherd-sdk/src/cow/*.rs` (AppData resolver, structured error envelope)
- `docs/operations/{e2e-testnet-runbook,production}.md`
- `docs/operations/load-reports/*.md`
- `scripts/{e2e-onchain,load-run,load-bootstrap,load-teardown,_twap_calldata}.{sh,py}`
- `tools/{load-gen,orderbook-mock}/**`

Closes COW-1029, COW-1030, COW-1032, COW-1033, COW-1034, COW-1035, COW-1036, COW-1064, COW-1071, COW-1072, COW-1073, COW-1074, COW-1075, COW-1076, COW-1077, COW-1079, COW-1080.

Linear milestone: [M4 - production hardening + E2E](https://linear.app/bleu-builders/project/shepherd). Companion: M3 (`dev/m3`).

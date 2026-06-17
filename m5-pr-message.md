# M5 epic: multi-chain deploy + packaging + docs reconciliation

## Summary

Builds on M4 (`dev/m4`). M5 closes out the grant: packages the M4 daemon for operators (Docker + ghcr CI), adds the pre-soak backtest harness + baseline-latency tooling, lands the small protocol-side hardening items surfaced during M4 soak runs, and reconciles the docs across M2-M5 so the on-disk story matches the shipped behaviour.

## Core deliverable

| Area | What landed |
|---|---|
| Docker + compose + ghcr CI | `Dockerfile` (multi-stage rust build), `docker-compose.yml` for the daemon + scrape stack, GHCR push on tag. |
| Pre-soak backtest harness | `shepherd-backtest` crate + `tools/backtest-collect/` replay a 7-day Sepolia EthFlow window before soak runs, giving an offline regression bar for module behaviour (COW-1078). |
| Baseline-latency tool | `tools/baseline-latency/` measures TWAP-relayer PUT and EthFlow indexer ingest latencies across 5 chains so soak reports can attribute regressions to the right lane (COW-1084). |
| Chain-forward revert data | `engine_config.rs` + `host/impls/chain.rs` forward `eth_call` `ErrorResp.data` into `HostError.data` so module code can decode `IConditionalOrder` reverts the same way it decodes orderbook errors (COW-1082). |
| Backoff retry cap | `ethflow-watcher` caps `backoff:{uid}` retries at `MAX_BACKOFF_RETRIES` so a permanently-failing orderbook submission stops eating fuel (COW-1083). |
| TWAP skip submit_order on submitted UID | `twap-monitor` consults `submitted:{uid}` before re-submitting, preventing duplicate orderbook posts on supervisor restart (COW-1085). |
| Event-loop log block-stream gap closures | `runtime/event_loop.rs` logs the WS-reconnect gap at `Info`, giving operators a visible signal during reconnects (COW-1086). |
| Env-var substitution + fail-fast HTTP rpc_url + RPC key redaction | `engine_config.rs` resolves `${VAR}` placeholders in `engine.toml`; HTTP `rpc_url` configs fail-fast at boot; API keys in RPC URLs are redacted from boot logs. |
| Rust-idiomatic compliance pass | M5 compliance sweep applied to the M4 + M5 surface area. |
| Docs reconciliation across M2-M5 | `docs/` updated end-to-end: ADR statuses, deployment guide, e2e runbook, operations guides re-flowed to match shipped behaviour. |

## Validation

- `cargo fmt --all -- --check` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo test --workspace` — **249 tests passing**; backtest harness has its own integration tests.
- Docker image builds cleanly via `docker build .`; compose stack boots end-to-end against a local Anvil + RPC + orderbook-mock.

## M5-specific paths for review

- `Dockerfile`, `docker-compose.yml`, `.github/workflows/docker.yml`
- `crates/shepherd-backtest/`
- `tools/backtest-collect/`, `tools/baseline-latency/`
- `crates/nexum-engine/src/engine_config.rs` (env-var substitution, fail-fast HTTP, key redaction)
- `crates/nexum-engine/src/host/impls/chain.rs` (forward `eth_call` ErrorResp.data)
- `modules/ethflow-watcher/src/strategy.rs` (observe+verify redesign, backoff cap)
- `modules/twap-monitor/src/strategy.rs` (skip-submitted-uid)
- `crates/nexum-engine/src/runtime/event_loop.rs` (WS reconnect gap log)
- `docs/` reconciliation sweep across the M2-M5 surface

Closes COW-1078, COW-1082, COW-1083, COW-1084, COW-1085, COW-1086.

Linear milestone: [M5 - multi-chain deploy + docs](https://linear.app/bleu-builders/project/shepherd). Companion: M4 (`dev/m4`).

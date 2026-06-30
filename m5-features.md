# M5 Features & Changes (dev/m5-base vs dev/m4-rebase)

Diff summary: 67 files changed, 14841 insertions(+), 1157 deletions(-)
M5-unique commits: 19 (excluding 1 merge commit)

## 1. Docker Deployment (M5 deployment)

### Dockerfile + Compose + GHCR CI
- New `Dockerfile` — multi-stage build for nexum-engine
- New `docker-compose.yml` — orchestration with healthchecks
- New `.dockerignore`
- New `.github/workflows/docker.yml` — GHCR CI pipeline (88 lines)
- New `engine.docker.toml` — Docker-specific engine config
- New `docs/deployment/docker.md` — Docker deployment documentation (195 lines)
- New `docs/deployment/prometheus.yml` — Prometheus scrape config
- New `.env.example` — environment variable template (25 lines)

### Deploy fixes
- `chore(deploy): gitignore /engine.toml to protect operator RPC keys`
- `fix(deploy): healthcheck uses bash /dev/tcp (wget not in runtime image)`
- `fix(deploy): healthcheck must invoke bash explicitly (CMD-SHELL is dash)`

## 2. Engine Config Enhancements

### `${VAR}` env-var substitution in engine.toml (for RPC URLs)
- `crates/nexum-engine/src/engine_config.rs` — env-var substitution logic (+164 lines)
- `engine.example.toml` — updated to use `${RPC_URL_*}` variables
- `engine.docker.toml` — uses env-var substitution

### Fail-fast on HTTP rpc_url + redact API keys in boot logs
- `crates/nexum-engine/src/engine_config.rs` — URL validation + redaction (+188 lines)
- `crates/nexum-engine/src/host/provider_pool.rs` — related changes
- `crates/nexum-engine/src/main.rs` — boot log adjustments
- `docs/production.md` — updated (+11 lines)
- `engine.example.toml` — updated comments

## 3. Forward eth_call ErrorResp.data into HostError.data (COW-1082)

- `crates/nexum-engine/src/host/impls/chain.rs` — major rewrite (+149/-15 lines)
- `crates/nexum-engine/src/host/provider_pool.rs` — error data forwarding (+60 lines)
- `modules/twap-monitor/src/strategy.rs` — consume forwarded error data

## 4. EthFlow Watcher Improvements

### Cap backoff retries at MAX_BACKOFF_RETRIES (COW-1083)
- `modules/ethflow-watcher/src/strategy.rs` — +168 lines, retry cap logic

### Observe + verify redesign
- `deploy: ethflow-watcher observe + verify redesign rebased onto M5`
- `modules/ethflow-watcher/src/strategy.rs` — major refactor (1015 lines changed: +243/-785)
- `modules/ethflow-watcher/src/lib.rs` — updated module structure

### Drop bogus wildcard arm
- `fix(ethflow-watcher): drop bogus wildcard arm from observe_placement`

## 5. TWAP Monitor: Skip Duplicate Submissions (COW-1085)

- `fix(twap-monitor): skip submit_order when submitted:{uid} already in store`
- `modules/twap-monitor/src/strategy.rs` — +141 lines, dedup via local-store check

## 6. Backtest: Pre-soak EthFlow Replay Harness (COW-1078)

### shepherd-backtest crate
- New `crates/shepherd-backtest/Cargo.toml`
- New `crates/shepherd-backtest/src/main.rs` — CLI entry point (139 lines)
- New `crates/shepherd-backtest/src/fixtures.rs` — test fixture loading (109 lines)
- New `crates/shepherd-backtest/src/replay.rs` — replay engine (192 lines)
- New `crates/shepherd-backtest/src/report.rs` — report generation (237 lines)
- `modules/ethflow-watcher/Cargo.toml` — updated for backtest integration
- `modules/ethflow-watcher/src/lib.rs` — exports for backtest

### Backtest tooling
- New `tools/backtest-collect/backtest_collect.py` — Python fixture collector (578 lines)
- New `tools/backtest-collect/fixtures-2026-06-22.json` — fixture data (9873 lines)

### Backtest report
- New `docs/operations/backtest-reports/backtest-7d-2026-06-22.md` — 7-day backtest report (289 lines)

## 7. Baseline Latency Tool (COW-1084)

- New `tools/baseline-latency/baseline_latency.py` — EthFlow indexer creationDate semantics (828 lines)
- New `tools/baseline-latency/.gitignore`
- New `tools/baseline-latency/data/` — per-chain baseline data (sepolia, mainnet, gnosis, base, arbitrum_one)
- New `docs/operations/baselines/baseline-latency-2026-06-19.md` — baseline report (60 lines)

## 8. Event Loop: Block Stream Gap Closure Logging (COW-1086)

- `crates/nexum-engine/src/runtime/event_loop.rs` — +98 lines
- Logs block stream gap closures from alloy-internal reconnects

## 9. Rust Idiomatic: M5 Compliance Pass (#67)

- `chore(rust-idiomatic): M5 compliance pass (cherry-pick M4 + M5 deploy fixes)`
- 16 files changed, 202 insertions(+), 110 deletions(-)
- Touches: engine_config, chain host, provider_pool, event_loop, supervisor, backtest crate, SDK, modules, orderbook-mock

## 10. Documentation Reconciliation (#68)

- `chore(docs): reconcile vapor + capability-gating drift across M2-M5`
- 9 docs files updated: 00-overview, 01-runtime-environment, 02-modules-events-packaging, 05-sdk-design, 06-production-hardening, 07-rpc-namespace-design, 08-platform-generalisation, diagrams, migration

## 11. Minor Fixes & Chores

### strum derives
- `chore(nexum-engine): derive strum::IntoStaticStr on EnvVarError + FilterError`

### em-dash cleanup
- `chore(engine.*.toml): replace em-dashes with ASCII hyphens`

### SDK address refactor
- `refactor(shepherd-backtest): consume shepherd_sdk::address::AddressParse (audit JC5)`
- `crates/shepherd-sdk/src/address.rs` — extended AddressParse helper (+64 lines changed)

## Summary of New Files

```
.dockerignore
.env.example
.github/workflows/docker.yml
Dockerfile
docker-compose.yml
engine.docker.toml
docs/deployment/docker.md
docs/deployment/prometheus.yml
docs/operations/backtest-reports/backtest-7d-2026-06-22.md
docs/operations/baselines/baseline-latency-2026-06-19.md
crates/shepherd-backtest/Cargo.toml
crates/shepherd-backtest/src/fixtures.rs
crates/shepherd-backtest/src/main.rs
crates/shepherd-backtest/src/replay.rs
crates/shepherd-backtest/src/report.rs
tools/backtest-collect/backtest_collect.py
tools/backtest-collect/fixtures-2026-06-22.json
tools/baseline-latency/.gitignore
tools/baseline-latency/baseline_latency.py
tools/baseline-latency/data/arbitrum_one.json
tools/baseline-latency/data/base.json
tools/baseline-latency/data/gnosis.json
tools/baseline-latency/data/mainnet.json
tools/baseline-latency/data/sepolia.json
```

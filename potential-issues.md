# Shepherd Code Review: Potential Issues

**Last updated:** 2026-06-30 (verified against `dev/m5` branch)
**Original review:** 2026-06-29 (`dev/m5-base`, SHA `9ad747e`)

---

## RFP Issues

Issues where the implementation diverges from the grant application's stated deliverables.

### 1. ComposableCoW smart contract modifications not delivered [M2]

**Grant says (M2):** "Smart contract modifications to ComposableCoW/TWAP handler: Enhanced polling interfaces for efficient order discovery, Optimised getter functions for active TWAP parts, Events for better monitoring capabilities"

**Reality:** No Solidity modifications were made. The TWAP module uses raw `eth_call` to `getTradeableOrderWithSignature` and ABI-decodes the result in the guest module, with SDK helpers in `shepherd-sdk`.

**Rationale (ADR-0006):** Documented as an intentional design choice -- contract mods would "put a single concrete TWAP implementation behind a WIT boundary" and prevent competing strategies. The eth_call + SDK approach is more flexible.

**Assessment:** The functional goal is achieved (orders are discovered and polled), but the grant deliverable as literally written was not delivered. This should be explicitly called out in milestone reporting.

### 2. No evidence of 7-day unattended soak test [M4]

**Grant says (M4 success criteria):** "System runs for 7 days on testnet without manual intervention"

**Reality:** The closest artifact is `docs/operations/backtest-reports/backtest-7d-2026-06-22.md`, but this is a pre-soak backtest that replays 7 days of collected events through MockHost in a single run, not a 7-day unattended live test. The report states: "Soak (COW-1031) is unblocked from the backtest side; remaining blockers are external (paid RPC + VM for the wall-clock run)."

**Assessment:** The infrastructure for long-running tests exists, but the 7-day success criterion is not verifiably demonstrated in accessible artifacts.

### 3. 48-hour uptime test not evidenced [M2]

**Grant says (M2 success criteria):** "100% uptime over 48-hour test period"

**Same gap as above.** Load test reports in `docs/operations/load-reports/` cover 2-minute runs. The E2E report covers a 1h 23m run. No 48-hour report found.

---

## Safety Issues

### 1. `getrandom` failure silently returns zero-filled buffer [M1] — COW-1097

**File:** `crates/nexum-engine/src/host/impls/random.rs:10-15`

```rust
async fn fill(&mut self, len: u32) -> Vec<u8> {
    let mut buf = vec![0u8; len as usize];
    let _ = getrandom::fill(&mut buf);  // error silently dropped
    buf
}
```

If `getrandom::fill()` fails (extremely rare on supported platforms), the module receives all zeros instead of random bytes. If a module uses this for nonces, salts, or key material, this is a cryptographic weakness.

**Severity:** Low (failure is extremely rare on Linux/macOS), but the silent swallowing of the error is a code smell. Should either propagate the error via `HostError` or log a warning.

### 2. No RPC method allowlist on chain requests [M2]

**File:** `crates/nexum-engine/src/host/impls/chain.rs:12-35`

Modules can call any JSON-RPC method via `chain::request(chain_id, method, params)`. There is no allowlist. The provider pool's doc comment explicitly states: "No method allowlist, no re-encoding of params." If the engine is configured with an RPC endpoint that exposes signing methods (`eth_sign`, `eth_signTransaction`, `personal_unlockAccount`), a malicious module could sign arbitrary transactions.

**Mitigation:** In practice, production RPC endpoints (Alchemy, Infura, DRPC) are read-only and don't expose signing. This is an operational concern, not a code bug.

**Severity:** Medium. The engine should at minimum log a warning if dangerous methods are called, or document that operators must use read-only RPC endpoints.

### 3. HTTP-only RPC with block/log subscriptions — partially addressed [M4] — COW-1092

**File:** `crates/nexum-engine/src/engine_config.rs`

**Original issue:** When an engine config uses an HTTP URL for a chain that has modules subscribed to blocks or logs, the engine emitted a warning but proceeded. At runtime, `eth_subscribe` would fail.

**What changed:** The `validate_transports()` method was upgraded from `warn!` to `tracing::error!`, the log message is more detailed with a suggested WebSocket URL, and there is a `require_ws` config flag (default `true`).

**What remains:** The validator still only logs the error -- it does not return `Err(...)` or abort the boot process. The event loop will still start and the subscription will fail at runtime. The test `validate_runs_without_panicking_on_http_url` confirms the validator's contract is "log + continue", not "abort".

**Severity:** Medium. Should be elevated to a hard error when modules have block/log subscriptions on that chain.

### 4. Four host capabilities are stubs returning `Unsupported` [M1]

**Files:**
- `crates/nexum-engine/src/host/impls/identity.rs` -- `sign()`, `sign_typed_data()` return `Unsupported`
- `crates/nexum-engine/src/host/impls/messaging.rs` -- `publish()` returns `Unsupported`
- `crates/nexum-engine/src/host/impls/remote_store.rs` -- all four functions return `Unsupported`
- `crates/nexum-engine/src/host/impls/http.rs` -- `fetch()` returns `Unsupported` (allowlist check does work)

**Severity:** Low for current use cases (TWAP and EthFlow don't need these). These are documented as 0.3 scope. However, modules that declare these as `required` capabilities will pass the manifest check at load time but fail at runtime -- the capability enforcement validates that the WIT import exists, not that the host implementation works.

---

## Missing Tests

### High Priority

**1. WS reconnection integration test [M4] — COW-1100, COW-1087**
- **File:** `crates/nexum-engine/src/runtime/event_loop.rs:76-114`
- **Gap:** The reconnect logic with exponential backoff has no integration test that verifies a dropped WebSocket connection is recovered. The `HEALTHY_WINDOW` (60s) backoff reset is untested. COW-1100 goes further: events during the down-window are silently lost (no `eth_getLogs` backfill on resubscribe). COW-1087 notes that log subscription gap closures are not observed either.
- **Why it matters:** This is the primary durability mechanism for production operation.

**2. Event ordering guarantees [M4]**
- **File:** `crates/nexum-engine/src/runtime/event_loop.rs:289-402`
- **Gap:** No tests verify that block events from multiple chains are interleaved correctly, or that log + block events don't starve each other under the `select_all` arbitration.
- **Why it matters:** Modules assume ordered delivery. Out-of-order events could cause incorrect state transitions.

### Medium Priority

**3. HTTP allowlist bypass attempts — partially addressed [M1] — COW-1096 (Done)**
- **File:** `crates/nexum-engine/src/host/impls/http.rs`
- **What changed:** The `extract_host` function was rewritten to use `url::Url::parse` (proper RFC 3986 handling), fixing the original COW-1096 bug where query strings/fragments containing `/` could bypass the allowlist. Tests cover standard URLs, user-info+port, query params, non-http schemes, malformed input, and subdomain isolation.
- **What remains:** No dedicated SSRF-style bypass attempt tests (e.g., `http://allowed.com@evil.com`, backslash in host). The `http.rs` host implementation itself has no test module.

**4. Graceful shutdown — partially addressed [M4]**
- **File:** `crates/nexum-engine/src/runtime/event_loop.rs:419-436`
- **What changed:** The `event_loop::run` function implements graceful shutdown (COW-1072): drops stream receivers, awaits `tasks.shutdown()`, logs final stats. The supervisor tests cover the shutdown timer path.
- **What remains:** No test verifies that in-flight `call_on_event` dispatches complete before shutdown, or that reconnect tasks exit cleanly when receivers drop.

**5. MockHost fidelity gaps — partially addressed [M3]**
- **File:** `crates/shepherd-sdk-test/src/lib.rs`
- **What changed:** MockHost now includes `MockChain` with call recording and response programming, `MockLocalStore` with full CRUD, `MockCowApi` with `submit_order` and `cow_api_request`, and `MockLogging` with line buffering and filtering.
- **What remains:** No store size limits, no namespace isolation (MockLocalStore is a flat HashMap unlike the real redb store), no error injection capability.

### Lower Priority

**6. EthFlow verification flow error paths — partially addressed [M2] — COW-1099**
- **File:** `modules/ethflow-watcher/src/strategy.rs`
- **What changed:** Tests now cover 200 (observed marker written), 404 (no marker, will recheck on re-delivery), and generic non-404 errors (502 bad gateway -> Warn log, no marker).
- **What remains:** No explicit tests for 429 (rate limit) or malformed JSON responses. The root cause (COW-1099: cow-api REST passthrough discards HTTP status) remains open.

---

## Other Issues

### 1. Documentation references wrong manifest filename [M1]

**File:** `docs/02-modules-events-packaging.md:76`

The ASCII tree example on line 76 still shows `nexum.toml` as the module manifest filename. The prose and header correctly say `module.toml`, but the diagram was never updated. Developers following the diagram will create the wrong file.

### 2. Documentation describes manifest schema features that don't exist [M2]

**Discrepancies found:**

| Docs say | Code does | Impact |
|----------|-----------|--------|
| `[chains].optional = [...]` in manifest | No `chains` field in `Manifest` struct at all; chain IDs are per-subscription | Developers may expect optional chain support |
| `topics = ["0x..."]` (array) in log subscriptions | Code uses `event_signature: Option<String>` (single optional string) | Developers may try multi-topic filters |
| `[capabilities.identity].methods` | No identity sub-config in parser | Minor, identity is stubbed anyway |

### 3. Resource limits partially configurable [M4] — COW-1093

**File:** `crates/nexum-engine/src/supervisor.rs`

**Original issue:** Despite the manifest schema supporting `[module.resources]`, the engine ignored these values and used hardcoded constants.

**What changed:** Resource limits are now configurable at the engine config level via `[limits]` in `engine.toml`. The `ModuleLimits` struct supports optional `fuel_per_event` and `memory_bytes` fields with defaults (1B instructions, 64 MiB). The supervisor passes `&engine_cfg.limits` into `boot_module`.

**What remains:** Limits are global, not per-module. The manifest parser has no resource limit fields. Per-module overrides are documented as 0.3 scope. COW-1093 specifically flags the local-store side: no per-module storage quota, so a single module can exhaust shared disk.

### 4. `query-module` WIT world defined but completely unimplemented [M1]

**File:** `wit/nexum-host/query-module.wit`

The `query-module` world (request/response pattern with `evaluate` function) is defined in the WIT package and marked as EXPERIMENTAL. No host implementation exists. A module compiled against this world will fail to instantiate.

**Risk:** Low -- clearly marked as experimental. But its presence in the published WIT package could confuse module authors.

---

## Triage Issues

Open issues from the CoW triage queue, ranked by criticality. Issues already covered in the sections above are cross-referenced; standalone triage issues are described below.

### High Criticality

**1. COW-1104 — Backtest harness validates deleted submit-based strategy [M4]**
- **Priority:** High
- **Summary:** The `shepherd-backtest` replay harness was written against the old submit-based ethflow-watcher strategy. After the COW-1076 redesign to observe+verify, the harness and its 7d report are stale and unreproducible. Blocks COW-1078 backtest sign-off.
- **URL:** https://linear.app/bleu-builders/issue/COW-1104

**2. COW-1099 — cow-api REST passthrough discards HTTP status [M4]**
- **Priority:** High
- **Summary:** `cow-api::request` returns the response body as `Ok(_)` for every HTTP status. It never inspects `response.status()`, so 4xx/5xx (including 404) is indistinguishable from 200 at the WIT boundary. This breaks EthFlow's observe flow (false `observed:` markers on 404).
- **Cross-ref:** Missing Test #6 (EthFlow error paths)
- **URL:** https://linear.app/bleu-builders/issue/COW-1099

**3. COW-1100 — WS reconnect loses every event in the down-window [M4]**
- **Priority:** High
- **Summary:** No `eth_getLogs` backfill after WS reconnection. Events during the down-window are silently lost.
- **Cross-ref:** Missing Test #1 (WS reconnection)
- **URL:** https://linear.app/bleu-builders/issue/COW-1100

**4. COW-1097 — `random::fill` silently returns zero-filled bytes on CSPRNG failure [M1]**
- **Priority:** High
- **Cross-ref:** Safety Issue #1
- **URL:** https://linear.app/bleu-builders/issue/COW-1097

### Medium Criticality

**5. COW-1091 — No HTTP timeout on the CoW orderbook client [M2]**
- **Priority:** Medium
- **Summary:** Outbound HTTP to the CoW orderbook has no timeout configured (`reqwest` defaults to no timeout), so a slow or hung endpoint can block a module's event dispatch indefinitely.
- **File:** `crates/nexum-engine/src/host/cow_orderbook.rs`
- **URL:** https://linear.app/bleu-builders/issue/COW-1091

**6. COW-1092 — No per-request timeout on chain JSON-RPC calls [M2]**
- **Priority:** Medium
- **Summary:** The alloy provider in `ProviderPool` has no per-request timeout, so a slow or stuck RPC node hangs the `chain.request` call. Same failure mode as COW-1091 but on the chain side.
- **Cross-ref:** Safety Issue #3 (HTTP-only RPC)
- **URL:** https://linear.app/bleu-builders/issue/COW-1092

**7. COW-1105 — `redact_url` leaks RPC secrets for non-standard URL shapes [M5]**
- **Priority:** Medium
- **Summary:** The URL redaction heuristic only handles `/v2/<longkey>` shapes. Credential-in-URL (`user:pass@host`), dotted/JWT tokens, and short keys are leaked to logs. RPC-key redaction is a headline M5 feature with real bypass classes.
- **URL:** https://linear.app/bleu-builders/issue/COW-1105

**8. COW-1095 — No test asserting subscription topic-0 == keccak256(event signature) [M2]**
- **Priority:** Medium
- **Summary:** Log subscriptions pin their event via a hardcoded topic-0 in `module.toml`. No test verifies these hardcoded values match `keccak256` of the canonical event signature. A typo or ABI change would silently miss all events.
- **URL:** https://linear.app/bleu-builders/issue/COW-1095

**9. COW-1087 — Log subscription gap closures not observed [M4]**
- **Priority:** Medium
- **Summary:** COW-1086 added gap closure logging for block subscriptions, but not for log subscriptions. Missed log events after a WS reconnect are invisible to operators.
- **Cross-ref:** Missing Test #1 (WS reconnection)
- **URL:** https://linear.app/bleu-builders/issue/COW-1087

**10. COW-1005 — Support ComposableCoW v2 `ConditionalOrderRemoved` event [M2]**
- **Priority:** Medium
- **Summary:** Feature request: index the new `ConditionalOrderRemoved` event from the updated ComposableCoW contract to clean up watches when orders are removed on-chain.
- **URL:** https://linear.app/bleu-builders/issue/COW-1005

**11. COW-1093 — Local-store has no per-module size/quota limit [M2]**
- **Priority:** Medium
- **Summary:** All modules share a single redb `Database`. `set()` does no size accounting, so a single module can write unbounded keys/values and exhaust disk for all modules.
- **Cross-ref:** Other Issue #3 (Resource limits)
- **URL:** https://linear.app/bleu-builders/issue/COW-1093

### Low Criticality

**12. COW-1098 — Manifest capability allowlist does not cover the WASI surface [M1]**
- **Priority:** Low
- **Summary:** Engine links full WASI p2 (`wasi:sockets`, `wasi:filesystem`, etc.) via `wasmtime_wasi::p2::add_to_linker_async` but the manifest capability gate only checks `nexum:host` interfaces. A module could import WASI interfaces without declaring them.
- **URL:** https://linear.app/bleu-builders/issue/COW-1098

**13. COW-1101 — Engine runs with only dead-module subscriptions [M4]**
- **Priority:** Low
- **Summary:** If all modules fail init, the engine still opens live RPC subscriptions and runs an empty event loop instead of exiting or warning loudly. `block_chains()` and `log_subscriptions()` walk every loaded module with no filter on the `alive` flag.
- **URL:** https://linear.app/bleu-builders/issue/COW-1101

**14. COW-1102 — balance-tracker false alert on first block [M3]**
- **Priority:** Low
- **Summary:** `check_one` defaults prior balance to `U256::ZERO`, so the first block always triggers a "balance changed" alert. The code comment acknowledges this but the implementation contradicts its own stated goal.
- **URL:** https://linear.app/bleu-builders/issue/COW-1102

**15. COW-1103 — twap-monitor orphaned gate markers leak [M2]**
- **Priority:** Low
- **Summary:** `next_block:`/`next_epoch:` gate keys are not cleaned up when a watch fails to decode in `poll_all_watches`, accumulating dead keys in the store. Normal `DropWatch` cleanup works correctly; the leak is specific to the decode-failure path.
- **URL:** https://linear.app/bleu-builders/issue/COW-1103

---

## Resolved Since Last Review

Items removed from this document because they were addressed in the `dev/m5` branch:

| Item | Resolution |
|------|------------|
| RFP: EthFlow module redesigned (observe+verify instead of submit) [M2] | Acknowledged design decision per COW-1076. Module header and docs explicitly state "observes and verifies, does not submit." |
| RFP: Proc macros described in SDK docs do not exist [M3] | `docs/05-sdk-design.md` now has a banner marking it as "0.3+ north-star vision" with a shipped-vs-deferred feature table. |
| Missing Test: Simultaneous multi-module crash behavior [M4] | Supervisor tests now cover fuel-bomb + healthy module side-by-side and cross-chain poison quarantine. |
| Missing Test: CoW API error variant coverage [M2] | Tests now cover 4xx/5xx responses, network errors, dead server, invalid JSON, unknown methods, malformed paths. |
| Missing Test: RPC error edge cases [M2] | Tests now cover revert data forwarding, transport failures, out-of-range codes, unknown chains, invalid params. |
| Missing Test: Local store concurrent access [M2] | Tests now cover 8-thread writes, concurrent reads during writes, list_keys racing with delete, stress tests. |
| Missing Test: Poison pill edge cases [M4] | Supervisor tests cover full escalation path (trap threshold -> quarantine -> skip) and cross-chain isolation. |
| Missing Test: Module state corruption recovery [M2] | `DropWatch` now cleans up `next_block:`/`next_epoch:` gate keys. Tests verify cleanup on `OrderNotValid` revert and permanent submit errors. |
| Other: SDK design doc aspirational [M3] | Banner added with shipped-vs-deferred feature table and pointers to actual API docs. |
| Other: Module boilerplate duplication [M3] | `bind_host_via_wit_bindgen!()` macro reduced per-module boilerplate to ~6 lines of inherently module-specific code. |
| COW-1096: `extract_host` mis-parses URL host [M1] | Fixed: `extract_host` now uses `url::Url::parse` for proper RFC 3986 handling. Marked Done in Linear. |

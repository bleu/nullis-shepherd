---
status: proposed
implemented-in: bleu/nullis-shepherd#12, #13, #15, #22, #23, #24, #25
---

# M3 Host trait surface: four per-capability traits + supertrait `Host`, with per-module `strategy.rs` / `lib.rs` split

## Context

`docs/05-sdk-design.md` describes a much richer M5+ SDK (`#[nexum::module]` proc macro, alloy `Provider`, `TypedState`, `Signer`, named event handlers with async dispatch). M3's scope was narrower: deliver a testable host abstraction that lets module logic compile against an in-memory mock without a `wasm32-wasip2` toolchain, and that the M2 modules (twap-monitor, ethflow-watcher) can adopt without breaking their existing dispatch.

The constraint is unusual: `wit_bindgen::generate!` emits per-cdylib types - every module gets its own `HostError`, `Event`, `Log`, etc. - so a single shared SDK type cannot be re-used across the wit boundary. Mocks live in their own crate (`shepherd-sdk-test`) and need to compile for the host target (not wasm).

## Decision

Three coupled choices:

### 1. Four per-capability traits with a supertrait `Host`

`shepherd-sdk` exposes four traits, one per host import:

```rust
pub trait ChainHost     { fn request(&self, chain_id: u64, method: &str, params: &str) -> Result<String, HostError>; }
pub trait LocalStoreHost { fn get / set / delete / list_keys ... }
pub trait CowApiHost    { fn submit_order(&self, chain_id: u64, body: &[u8]) -> Result<String, HostError>; }
pub trait LoggingHost   { fn log(&self, level: LogLevel, message: &str); }

pub trait Host: ChainHost + LocalStoreHost + CowApiHost + LoggingHost {}
impl<T: ChainHost + LocalStoreHost + CowApiHost + LoggingHost> Host for T {}
```

Module strategy code takes `&impl Host` (or `<H: Host>`), so it can call any of the four interfaces uniformly. Tests inject `shepherd_sdk_test::MockHost`; production inject `WitBindgenHost`. The blanket `impl<T: ...> Host for T` means callers never write `impl Host for MyHost {}` by hand.

### 2. SDK-side `HostError` mirroring the wit struct field-for-field

`shepherd_sdk::host::HostError` has the same fields as the wit-bindgen-generated `HostError` in each module crate, but is its own type:

```rust
pub struct HostError {
    pub domain: String,
    pub kind: HostErrorKind,
    pub code: i32,
    pub message: String,
    pub data: Option<String>,
}
```

Each module's `lib.rs` writes a one-liner `convert_err` and `sdk_err_into_wit` to bridge the two. The traits stay world-neutral: `shepherd-sdk-test` compiles for the host target without needing a wasm toolchain, and the mocks are usable from any module's tests.

### 3. Per-module `strategy.rs` + `lib.rs` split

Every module is shaped as:

- `strategy.rs` - pure logic. Imports `shepherd_sdk::host::{Host, HostError, LogLevel}`. Defines small carrier types (`LogView<'a>`, `BlockInfo`, `Settings`) so the strategy is wit-independent. Tests live here under `#[cfg(test)]` against `MockHost`.
- `lib.rs` - per-cdylib glue. `wit_bindgen::generate!`, the `WitBindgenHost` struct implementing all four traits with `chain::request` / `local_store::*` / `cow_api::submit_order` / `logging::log` calls, the `convert_err` + `sdk_err_into_wit` + `convert_level` helpers, and the `Guest` impl that destructures `types::Event` and delegates to `strategy`.

Reference implementations: `modules/examples/price-alert/`, `modules/examples/stop-loss/`, `modules/twap-monitor/`, `modules/ethflow-watcher/`. The wit-bindgen adapter is intentionally mechanical and is a candidate for a future declarative macro in `shepherd-sdk` (the `#[nexum::module]` design in doc 05).

## Considered options

- **Single fat `Host` trait.** Rejected: pulls every module's tests into mocking the full surface even when the strategy only touches one or two capabilities. The four-trait split lets tests `respond_to` exactly the calls the strategy makes.
- **`#[nexum::module]` proc macro now.** Rejected for M3 scope. The proc macro is the right shape long-term (see doc 05) but adds a macro crate, parsing logic, and a debugging surface we did not need to ship M2 modules with MockHost coverage. The manual adapter is verbose but understandable in one read; we land the macro as M5 work.
- **Re-export wit-bindgen `HostError` from the SDK.** Rejected: the wit-bindgen types are per-cdylib. Re-exporting one module's `HostError` would break all others. A shared SDK struct with field-equivalent shape and module-local `From` impls is the only way the SDK stays world-neutral.
- **Strategy lives in `lib.rs` next to the wit-bindgen adapter.** Rejected after BLEU-851 (price-alert) showed the dispatch matrix was not unit-testable without MockHost, and BLEU-854 / BLEU-855 ported twap-monitor / ethflow-watcher to the split. The wit-bindgen adapter is ~150 lines of mechanical glue; the strategy is hundreds of lines of logic - colocating them obscures both.

## Consequences

- **Strategy code is testable in native Rust** without `wasm32-wasip2`. The 145 host tests across the workspace (twap 20, ethflow 12, balance-tracker 13, price-alert 16, stop-loss 7, shepherd-sdk 27, shepherd-sdk-test 8, nexum-engine 41, plus 1 doctest) all exercise this seam.
- **The `WitBindgenHost` adapter is duplicated across modules.** ~150 lines of identical glue (the four trait impls plus the two converters and `convert_level`). Acceptable today; the M5 `#[nexum::module]` macro is the path to eliminate it.
- **`shepherd-sdk-test` does not need wit-bindgen.** It depends only on `shepherd-sdk` and `std`; no wasm toolchain involved. Tests compile and run as plain Rust.
- **`HostError` round-trips lossily at the WIT boundary.** The wit-bindgen and SDK types have identical fields today; if either evolves (new variant on `HostErrorKind`, new field), modules need a one-line `From` update. **Applied in M4 (COW-1029)**: `HostErrorKind` and `LogLevel` are `#[non_exhaustive]`; each module's `sdk_err_into_wit` and `convert_level` adapter carries a wildcard arm mapping unknown SDK-side variants to `HostErrorKind::Internal` / `Level::Info` respectively. `RetryAction` and `PollOutcome` stay exhaustive (domain-locked to the cow-rs `OrderPostErrorKind::is_retriable` and `IConditionalOrder` Solidity interfaces).
- **The four-trait split is not an interface contract with mfw78's WIT.** WIT defines the wire shape; the SDK traits are a Rust-side ergonomics layer. The two evolve together but are not the same artifact.
- **Future capabilities (e.g. `messaging`, `remote-store`, `http`) add new traits.** Each new host interface becomes a new trait + new `MockX` in `shepherd-sdk-test`, and the supertrait `Host` is bumped to bound on the new trait. Modules that do not use the new capability are unaffected (they only need `<H: ChainHost + LocalStoreHost>` etc. on the subset they actually touch - the supertrait is a convenience for full-surface modules, not a hard requirement).

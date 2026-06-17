# M1 consolidated: PRs #8, #9, #12, #15 with review

## Summary

This branch consolidates the features from PRs #8, #9, #12, and #15 into a single branch, with all late-cycle review feedback (mfw78, lgahdl) applied on top.

### Included PRs

| PR | Title | Status |
|---|---|---|
| #8 | runtime: implement cow-api, chain, local-store host backends | All review feedback applied |
| #9 | runtime: multi-module supervisor + block/log event loop | All review feedback applied |
| #12 | docs: ADR bundle (0001-0008) — engine and CoW architectural decisions | Fully included |
| #15 | chore(deps): patch cowprotocol to bleu/cow-rs main (post-alpha.3) | Fully included |

### Review feedback applied

The following refinements from mfw78's review (Jun 15-19) are included, which were pending on the individual PR branches:

1. **`ModuleStore`** — cached per-module keccak256 prefix. Hashing happens once in `LocalStore::module(name)`; every subsequent `get`/`set`/`delete`/`list_keys` concatenates without rehashing. `HostState` carries `ModuleStore` directly instead of `(LocalStore, module_namespace)`.

2. **Configurable `[limits]` in `engine.toml`** — `ModuleLimits` with optional `fuel_per_event` and `memory_bytes` fields that resolve against built-in defaults (1B fuel, 64 MiB memory). Replaces hardcoded constants.

3. **alloy bump 1.5 -> 1.8** for provider/transport/rpc crates; `alloy-primitives` stays at 1.6 (its own release cadence).

4. **`hex_encode` -> `alloy_primitives::hex::encode`** — removes the hand-rolled write-loop hex encoder.

5. **`ProviderPool::empty()` strict `#[cfg(test)]`** — was `cfg_attr(not(test), allow(dead_code))`.

6. **`manifest/error.rs` thiserror** — converted to `#[derive(thiserror::Error)]` with `#[from]` on `Io`/`Toml` variants.

7. **`extract_host` -> `url::Url::parse`** — replaces hand-rolled URL parser with `url::Url::parse` + `host_str()`, inheriting RFC 3986 handling.

### Additional commits (not in the original PRs)

- `chore(workspace): hoist [workspace.dependencies] + [workspace.lints]` — deduplicate dependency versions across workspace crates
- `feat(nexum-engine): migrate CLI from hand-rolled parser to clap` — applied before mfw78's review requested the same
- `docs(07-rpc-namespace-design): mark allowlist enforcement as future direction`

### Validation

```
cargo fmt --all --check                                    # clean
RUSTFLAGS=-D warnings cargo clippy -p nexum-engine --all-targets  # clean
cargo test -p nexum-engine                                 # 42 passed
```

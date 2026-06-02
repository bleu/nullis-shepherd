---
status: proposed
implemented-in: nullislabs/shepherd#8, nullislabs/shepherd#9
---

# Operator config (`engine.toml`) is separate from module manifest (`nexum.toml`)

## Context

The engine needs two distinct kinds of configuration: what the
**operator** decides at deployment time (which chains to connect to,
where the local-store database lives, which modules to boot) and
what the **module developer** declares at build time (required and
optional capabilities, HTTP allowlist, module-specific config keys).
These have different reviewers, different threat models, and change
on different cadences.

## Decision

Two distinct files, distinct schemas, distinct loaders:

- **`engine.toml`** — operator-owned, lives next to the engine binary
  or pointed to by `--engine-config`. Defines `[engine]` (state_dir,
  log_level), `[chains.<id>]` (rpc_url), and `[[modules]]` (path,
  manifest). Loaded by `engine_config::EngineConfig::load`.
- **`nexum.toml`** — module-developer-owned, ships in the module's
  bundle alongside its `.wasm` component. Defines `[module]`,
  `[capabilities]` (required, optional, http allowlist), `[config]`.
  Loaded by `manifest::load`.

The engine config carries the path to each module's manifest; the
two never collapse into one file.

## Considered options

- **Single `shepherd.toml` with `[engine]`, `[chains]`, `[[modules]]`
  *and* nested `[modules.<n>.capabilities]` per module.** Rejected:
  conflates operator and developer concerns. A module's capability
  declaration is a property of the build, not the deployment — it
  belongs in the artifact, not in the operator's local file. Auditing
  a module's capabilities also becomes a per-deployment exercise
  instead of a property visible in the published bundle.
- **`nexum.toml` inside the engine config (module entries embed it
  inline).** Rejected for the same reason; also bloats `engine.toml`.
- **Drop `engine.toml` entirely; pass everything as CLI flags or
  env vars.** Rejected: per-chain RPC URLs and module lists are
  awkward as flags, and `RUST_LOG` already covers the only thing
  that env vars naturally express.

## Consequences

- A deployment needs both files. A missing `engine.toml` falls back
  to "no chains, default state_dir" — the example logging module
  still runs; cow-api / chain backends report `unsupported`.
- A missing `nexum.toml` triggers the 0.1-compat deprecation warning
  in `manifest::fallback_manifest()` (defined in
  `crates/nexum-engine/src/manifest.rs`) and treats every linked
  capability as required. This fallback is scheduled for removal in
  0.3 per `docs/migration/0.1-to-0.2.md`.
- Module-bundle redistribution carries `nexum.toml` with the
  artifact; engines do not need to ship templates.
- Future content-addressed module distribution (0.3) embeds
  `nexum.toml` in the bundle hash; `engine.toml` references modules
  by content address rather than filesystem path. The split survives
  that migration unchanged.

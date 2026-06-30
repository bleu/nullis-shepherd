---
status: proposed
implemented-in: nullislabs/shepherd#8
---

# Per-module namespacing in `local-store` via 32-byte deterministic hash prefix

## Context

`nexum:host/local-store` is a key-value store shared across all modules the engine runs. Two modules using the same key string (e.g. `"last-block"`) must see disjoint values; one module must never read or overwrite another's data. The engine knows each module's identity at instantiation time, so namespacing is a host-side concern.

Two properties matter for the namespace prefix:

1. **Deterministic and unspoofable.** An arbitrary `module_name` string read out of `module.toml` lets a malicious or careless operator give two modules the same name and have one read the other's state. A fixed-size hash derived from the module's canonical identity is harder to collide and removes the operator-supplied-text attack surface.
2. **Composes with ENS-based module discovery** (per `docs/03-module-discovery.md`): when a module is identified by an ENS name (e.g. `twap-monitor.shepherd.eth`), the ENS namehash is a natural prefix. ENS TXT records pinning the `.wasm` content hash provide a separate verification path against the loaded bundle.

## Decision

Single redb database file at `EngineConfig.engine.state_dir`, single shared table `nexum:local-store`. Every key handed to redb is composed host-side as:

```
[32-byte namespace prefix][raw key bytes]
```

The 32-byte prefix is computed deterministically from the module's canonical identity:

- **ENS-identified modules** (M3+, per `docs/03`): prefix is `ens_namehash(name)` (EIP-137), e.g. `namehash("twap-monitor.shepherd.eth")`.
- **Locally-loaded modules** (current 0.2 scope, no ENS): prefix is `keccak256(module_name)` where `module_name` comes from `module.toml`'s `[module].name` field.

Both produce a 32-byte digest with the same domain, so a module loaded locally during development and later published under an ENS name can keep its existing state by registering an alias (`alias = keccak256(name)`) the engine recognises during the migration window. The exact alias mechanism is out of scope for this ADR.

Modules see plain key strings on both the read and write paths; the prefix is invisible to the WIT-facing API.

## Considered options

- **Separator string** (`{module}:{key}`). Rejected: any module name containing `:` collides with another module's `:`-bearing key. A fixed-size hash is unambiguous regardless of payload bytes.
- **`[len:u8][module_name][key]` length-prefixed string.** Rejected: spoofable (the name is operator-supplied text), and does not align with the ENS-based discovery path that 0.3 will introduce. The 32-byte hash is deterministic and namespace-uniform.
- **One redb database file per module.** Rejected: multiplies open file handles linearly in modules, blocks any future cross-module atomic operations (not currently planned but cheap to keep on the table), and complicates backup tooling (N files vs 1).
- **One redb *table* per module within a single file.** Rejected: redb `TableDefinition` lifetimes are `'static`, so table names must be known at compile time. Dynamic table opening per module would force string-leak workarounds and exposes the same name-collision question as separator-based keys.
- **Engine-allocated incrementing module id.** Rejected: stable across reboots only if the engine persists the allocation table, which adds a chicken-and-egg dependency on the local-store itself. Determinism from the name avoids the dependency entirely.

## Consequences

- The prefix is fixed-size (32 bytes) and independent of module name length. Range scans over a single module's keys are O(log n + module-key-count) — fine for our workload.
- Migrations changing the prefix derivation (e.g., switching the local-mode hash function or the ENS resolver) would orphan every existing module's persisted state. The derivation must stay stable through 0.x; ENS-mode introduction in 0.3 happens additively via the alias mechanism, not by changing existing prefixes.
- A module's `list-keys` iterates over the namespace range (32-byte prefix scan); the host strips the prefix before returning to the guest.
- Module data versioning (schema migrations across module versions) is the module's responsibility. The local-store does not version values; modules MAY embed a `schema_version` byte in their stored payloads and migrate on `init` when the read value's version differs from the current code's expectation.
- ENS-based discovery (per docs/03) integrates without a prefix-format change: when a module is loaded by ENS name, the prefix is `namehash(name)`. The corresponding `.wasm` content hash is verified via ENS TXT records before loading, separately from the local-store prefix derivation.
- Spoofing protection: an operator cannot make module A read module B's state by renaming, because the prefix is the hash of the canonical name. Renaming a module to match another's name produces a name conflict the engine refuses at boot, rather than silent state takeover.

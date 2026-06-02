---
status: proposed
implemented-in: nullislabs/shepherd#8
---

# Per-module namespacing in `local-store` via `[len:u8][module][key]` prefix

## Context

`nexum:host/local-store` is a key-value store shared across all modules
the engine runs. Two modules using the same key string (e.g.
`"last-block"`) must see disjoint values; one module must never read or
overwrite another's data. The engine knows each module's name at
instantiation time, so namespacing is a host-side concern.

## Decision

Single redb database file at `EngineConfig.engine.state_dir`, single
shared table `nexum:local-store`. Every key handed to redb is composed
host-side as:

```
[len: u8] [module_name: len bytes] [raw key: rest of the bytes]
```

Module names longer than 255 bytes are rejected at `LocalStore`
construction (matches the one-byte length prefix). Modules see plain
key strings on both the read and write paths; the prefix is invisible
to the WIT-facing API.

## Considered options

- **Separator string** (`{module}:{key}`). Rejected: any module name
  containing `:` collides with another module's `:`-bearing key. Length
  prefix is unambiguous regardless of payload bytes.
- **One redb database file per module.** Rejected: multiplies open
  file handles linearly in modules, blocks any future cross-module
  atomic operations (not currently planned but cheap to keep on the
  table), and complicates backup tooling (N files vs 1).
- **One redb *table* per module within a single file.** Rejected: redb
  `TableDefinition` lifetimes are `'static`, so table names must be
  known at compile time. Dynamic table opening per module would force
  string-leak workarounds and exposes the same name-collision question
  as separator-based keys.

## Consequences

- Module data is physically interleaved in the redb tree (range scans
  for one module's keys are O(log n + module-key-count) — fine for our
  workload).
- Migrations changing the namespacing layout break every existing
  module's persisted state. The format must stay stable through 0.x.
- A module's `list-keys` (when added) iterates over the namespace
  range; the host strips the prefix before returning to the guest.
- 255-byte module-name limit is enforced loudly at construction, so
  configuration errors surface at boot rather than silently corrupting
  data at first write.

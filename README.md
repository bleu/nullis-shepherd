# Shepherd

[![CI](https://github.com/nullisLabs/shepherd/actions/workflows/ci.yml/badge.svg)](https://github.com/nullisLabs/shepherd/actions/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL_3.0-blue.svg)](LICENSE)

Shepherd is the [CoW Protocol](https://cow.fi) distribution of **Nexum**, a WebAssembly Component Model runtime for secure, sandboxed execution of capability-scoped modules.

A module compiled against the universal `web3:runtime/headless-module` world runs on any Nexum-compatible host. A module compiled against `shepherd:cow/shepherd-module` additionally gains access to CoW Protocol APIs and order submission — and requires a Shepherd host.

## Why

- **Component Model from day 1** — WIT-defined API contract; structural sandboxing (no WASI, no FS, no network); multi-language guests.
- **Capability-scoped** — modules see only the host primitives they declare; nothing implicit.
- **Declarative subscriptions** — modules declare events in their manifest; the runtime wires sources.
- **Transactional state** — per-event all-or-nothing semantics; commit on success, rollback on trap.
- **Content-addressed distribution** — modules are fetched by hash (Swarm, IPFS, OCI, HTTPS); integrity always verified.
- **Self-hosted** — no centralised dependency; operator runs their own node.

## Layout

| Path | Purpose |
| --- | --- |
| `crates/nexum-runtime/` | Host runtime — wasmtime-based component loader and host implementations. |
| `modules/example/` | Reference guest module demonstrating the module ABI. |
| `wit/web3-runtime/` | Universal `web3:runtime` WIT package (csn, identity, local-store, remote-store, msg, logging). |
| `wit/shepherd-cow/` | `shepherd:cow` WIT package — CoW Protocol-specific extensions. |
| `docs/` | Architecture, design notes, and the universal primitive taxonomy. Start with [`docs/00-overview.md`](docs/00-overview.md). |

## Building

Shepherd uses [Nix](https://nixos.org/) flakes to pin the toolchain and [just](https://github.com/casey/just) as a task runner.

```sh
# Enter the dev shell (pulls Rust, wasm-tools, just, etc.)
nix develop

# Or with direnv:
direnv allow

# Build everything
just build

# Run the runtime against the example module
just run
```

Without Nix, you need: Rust (edition 2024, see `rust-toolchain.toml` if present), the `wasm32-wasip2` target, and `wasm-tools`.

## Documentation

The `docs/` directory contains the design corpus:

- [`00-overview.md`](docs/00-overview.md) — architecture, primitives, WIT worlds
- [`01-runtime-environment.md`](docs/01-runtime-environment.md) — host runtime
- [`02-modules-events-packaging.md`](docs/02-modules-events-packaging.md) — module ABI, events, packaging
- [`03-module-discovery.md`](docs/03-module-discovery.md) — static / ENS / on-chain registry
- [`04-state-store.md`](docs/04-state-store.md) — local + remote state
- [`05-sdk-design.md`](docs/05-sdk-design.md) — guest SDK
- [`06-production-hardening.md`](docs/06-production-hardening.md) — operational concerns
- [`07-rpc-namespace-design.md`](docs/07-rpc-namespace-design.md) — `csn` namespace
- [`08-platform-generalisation.md`](docs/08-platform-generalisation.md) — beyond CoW

## Contributing

Pull requests are welcome. Please open an issue first for substantial changes. CI runs `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test` against the workspace.

## License

[AGPL-3.0](LICENSE) © Nullis Labs LLC and contributors.

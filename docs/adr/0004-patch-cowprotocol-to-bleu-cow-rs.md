---
status: proposed
implemented-in: nullislabs/shepherd#10
---

# Patch `cowprotocol` crate to `bleu/cow-rs` main

## Context

`cowprotocol` v1.0.0-alpha.3 (the version on crates.io) was cut from `cowdao-grants/cow-rs` PR #5 at commit `1742ffa`. The published artifact predates 18 follow-up commits on `bleu/cow-rs` main that the engine materially depends on, in particular:

- `composable::Proof` byte-width fix (consumed by the TWAP poll path);
- `OrderCreation` zero-`from` fast-fail (closes a MEDIUM severity finding from mfw78's review of PR #5);
- `order_book` / `composable` submodule splits (cleaner imports on the engine side, no more `cowprotocol::order_book::*` re-export gymnastics).

ADR-0007 additionally commits us to pushing TWAP / EthFlow / app-data protocol logic upstream into `cowprotocol` first and consuming it via the same patched dependency, so the patch surface will continue growing through M2.

There is no published `alpha.4` and no scheduled date for one.

## Decision

Add a workspace-level `[patch.crates-io]` redirecting `cowprotocol` to `https://github.com/bleu/cow-rs` at commit `c012404`. Every crate that declares `cowprotocol = "1.0.0-alpha.3"` (engine, modules, future SDK) silently picks up the patched build with no `Cargo.toml` change at the dependent site.

## Considered options

- **Vendor the missing types locally.** Rejected: re-implementing `composable::Proof`, `OrderCreation`, etc. in the engine repo is exactly the AI-duplication anti-pattern mfw78 flagged in cow-rs PR #5. Reuse over reimplement applies.
- **Pin every dependent to `cow-rs` git directly.** Works but every new workspace member has to remember the git source. `[patch.crates-io]` centralises the override.
- **Wait for `alpha.4` to publish.** No ETA; the TWAP/EthFlow milestone cannot land without `composable::Proof` correct.

## Consequences

- `cargo update` will re-resolve to the same `rev` — the lock pins it.
- Bumping the rev is a single-line workspace edit; reviewers see one diff.
- Drop the patch entirely once a published `cowprotocol` release contains both the alpha.3 follow-ups and the ADR-0007 protocol-helper additions (`composable::poll_and_build_order`, `eth_flow::decode_placement`, `OrderPostError` rich variants). Until then, expect the patch rev to advance with every cow-rs merge that the engine consumes.
- Modules built against this workspace inherit the patch transitively; modules built standalone against crates.io will see `alpha.3` and may hit the very bugs the patch closes — flag in the SDK README when M3 lands.

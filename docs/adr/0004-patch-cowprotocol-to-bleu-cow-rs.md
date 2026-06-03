---
status: proposed
implemented-in: nullislabs/shepherd#10
---

# Patch `cowprotocol` crate to the head of upstream PR #5

## Context

`cowprotocol` v1.0.0-alpha.3 (the version on crates.io) was cut from an early snapshot of `cowdao-grants/cow-rs` PR #5 at commit `1742ffa`. That PR is still open and is the canonical upstream channel for landing additions to the Rust SDK. Its head branch is `bleu/cow-rs:main`, currently at commit `c012404`, carrying 18 follow-up commits the engine materially depends on:

- `composable::Proof` byte-width fix (consumed by the TWAP poll path).
- `OrderCreation` zero-`from` fast-fail (closes a MEDIUM severity finding from the PR #5 review).
- `order_book` / `composable` submodule splits (cleaner imports on the engine side).

ADR-0007 commits us to landing TWAP, EthFlow, and `OrderPostError` primitives into PR #5 directly, by pushing additional commits to its head branch. Each commit advances both PR #5 and the patch rev consumed here.

There is no published `alpha.4` and no scheduled date for one; the engine cannot wait.

## Decision

Add a workspace-level `[patch.crates-io]` redirecting `cowprotocol` to `https://github.com/bleu/cow-rs` at commit `c012404`. Every crate that declares `cowprotocol = "1.0.0-alpha.3"` (engine, modules, future SDK) silently picks up the patched build with no `Cargo.toml` change at the dependent site.

This is not a parallel fork. `bleu/cow-rs:main` IS the head branch of upstream PR #5. Pushing to it updates PR #5; the patch rev advances by bumping a single workspace line.

## Considered options

- **Vendor the missing types locally.** Rejected: re-implementing `composable::Proof`, `OrderCreation`, etc. in the engine repo is the AI-duplication anti-pattern flagged by reviewers on cow-rs PR #5. Reuse over reimplement applies.
- **Pin every dependent to `cow-rs` git directly.** Works but every new workspace member has to remember the git source. `[patch.crates-io]` centralises the override.
- **Open a separate PR per primitive against `cowdao-grants/cow-rs`.** Rejected: fragments review across multiple PRs when one already exists at the appropriate granularity. Stacking commits on PR #5 keeps the review thread coherent and lets reviewers track the cumulative change.
- **Wait for `alpha.4` to publish.** No ETA; the TWAP/EthFlow milestone cannot land without `composable::Proof` correct.

## Consequences

- `cargo update` will re-resolve to the same `rev`; the lock pins it.
- Bumping the rev is a single-line workspace edit; reviewers see one diff per primitive added to PR #5.
- Drop the patch entirely once a published `cowprotocol` release contains both the alpha.3 follow-ups and the ADR-0007 protocol-helper additions (`composable::poll_and_build_order`, `eth_flow::decode_placement`, `OrderPostError` rich variants). Until then, expect the patch rev to advance with every push to PR #5.
- Modules built against this workspace inherit the patch transitively; modules built standalone against crates.io will see `alpha.3` and may hit the very bugs the patch closes. Flag this in the SDK README when M3 lands.

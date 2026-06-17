//! # shepherd-sdk
//!
//! Guest-side SDK for Shepherd modules. The crate is the shared
//! companion to the per-module `wit_bindgen::generate!` invocation:
//! modules keep their own wit-bindgen call (which emits the world-
//! specific `Guest` trait, `HostError` shape, and host import shims
//! into the module's own crate) and pull helpers + canonical
//! primitive types from here.
//!
//! ## What lives here
//!
//! - [`prelude`] — `use shepherd_sdk::prelude::*` imports the
//!   protocol-level types modules need on every other line: alloy
//!   primitives ([`Address`](alloy_primitives::Address),
//!   [`B256`](alloy_primitives::B256),
//!   [`Bytes`](alloy_primitives::Bytes),
//!   [`U256`](alloy_primitives::U256), [`keccak256`](
//!   alloy_primitives::keccak256)) and cowprotocol's order /
//!   signing surface ([`OrderCreation`](cowprotocol::OrderCreation),
//!   [`OrderData`](cowprotocol::OrderData),
//!   [`OrderUid`](cowprotocol::OrderUid),
//!   [`OrderKind`](cowprotocol::OrderKind),
//!   [`Signature`](cowprotocol::Signature),
//!   [`Chain`](cowprotocol::Chain),
//!   [`GPv2OrderData`](cowprotocol::GPv2OrderData),
//!   [`EMPTY_APP_DATA_JSON`](cowprotocol::EMPTY_APP_DATA_JSON), and
//!   the [`ApiError`](cowprotocol::ApiError) +
//!   [`OrderPostErrorKind`](cowprotocol::error::OrderPostErrorKind)
//!   retry contract).
//!
//! - [`cow`] (BLEU-840) — `GPv2OrderData` <-> `OrderData` bridging,
//!   `IConditionalOrder` revert decoding, `RetryAction` classifier.
//!   Stubbed in this skeleton; populated by the BLEU-840 extraction.
//!
//! - [`chain`] (BLEU-840) — `eth_call` JSON plumbing
//!   (`eth_call_params`, `parse_eth_call_result`, `decode_revert_hex`).
//!   Stubbed in this skeleton; populated by the BLEU-840 extraction.
//!
//! - [`store`] (BLEU-840) — `WatchSet` and `BackoffLedger` helpers
//!   per ADR-0006. Stubbed in this skeleton.
//!
//! ## Why no `wit_bindgen::generate!` here
//!
//! The macro emits types into the calling crate (the module's
//! cdylib). Re-exporting wit-bindgen output from a library crate
//! would duplicate symbols and break the component-export contract.
//! Helpers in this SDK therefore take primitive types (`&[u8]`,
//! `Option<&str>`, slices) rather than the per-module `HostError`
//! struct; modules unpack their `HostError` on the way in. Trade-off
//! documented in ADR-0006 / ADR-0007 — the SDK stays on the guest
//! side, neutral to which world the module exports.

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod chain;
pub mod cow;
pub mod prelude;

/// `local-store` helpers: `WatchSet`, `BackoffLedger` per ADR-0006.
///
/// Skeleton — populated by a follow-up to BLEU-840 once a second
/// strategy module needs the same key conventions.
pub mod store {}

#[cfg(test)]
mod tests {
    //! The skeleton has no behaviour to exercise; this test just
    //! locks the prelude's surface — the build itself proves the
    //! re-exports compile against both `wasm32-wasip2` and the
    //! host target.

    use crate::prelude::*;

    #[test]
    fn prelude_re_exports_resolve() {
        let _addr: Address = Address::ZERO;
        let _hash: B256 = B256::ZERO;
        let _amt: U256 = U256::ZERO;
        let _empty: Bytes = Bytes::new();
        // cowprotocol re-exports
        let _kind: OrderKind = OrderKind::Sell;
        let _chain: Chain = Chain::Sepolia;
        assert_eq!(EMPTY_APP_DATA_JSON, "{}");
    }
}

//! `chain::request` JSON plumbing.
//!
//! Build the `[{to, data}, "latest"]` params array for `eth_call`,
//! parse the `"0x..."` hex result string, decode revert payloads from
//! the host's structured error data. Pure-logic helpers so a module
//! can plumb its own `chain::request` shim around them.

pub mod eth_call;

pub use eth_call::{decode_revert_hex, eth_call_params, parse_eth_call_result};

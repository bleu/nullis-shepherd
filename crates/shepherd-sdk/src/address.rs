//! Comma-separated EVM address-list parsing.
//!
//! Multiple Shepherd modules need to read a `[config]` value such as
//! `addresses = "0xabc..., 0xdef..."` and surface a typed error when
//! one of the entries is malformed. Each module previously rolled
//! its own `AddressListParseError` (balance-tracker, shepherd-backtest
//! after JC5 propagation). The shapes were identical; the audit
//! pass consolidates them here so future modules pick up the same
//! `Display` wording (operator-facing log strings stay stable) and
//! the same `#[non_exhaustive]` evolution guarantee.
//!
//! The parser stays deliberately permissive about whitespace + empty
//! trailing segments to match the wording operators have grown used
//! to (a literal trailing comma in `engine.toml` should not error).

use alloy_primitives::Address;

/// Typed errors returned by [`parse_address_list`]. Replaces the
/// `Result<_, String>` and per-module `AddressListParseError` /
/// `AddressParseError` shapes that previously lived in each
/// strategy crate (rubric prohibits stringly-typed errors).
///
/// The Display impls preserve the exact wording the previous
/// formatters produced so any operator-facing log strings remain
/// stable across the JC5 consolidation.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AddressParse {
    /// One of the comma-separated entries failed to parse as an
    /// EVM address.
    #[error("address #{index} ({raw:?}): {message}")]
    InvalidAddress {
        /// Zero-based position of the offending entry in the
        /// comma-separated list.
        index: usize,
        /// The trimmed source string that failed to parse.
        raw: String,
        /// Human-readable parse-error detail from
        /// `<Address as FromStr>::Err`.
        message: String,
    },
    /// The whole list was empty (or contained only whitespace +
    /// empty segments).
    #[error("expected at least one address")]
    Empty,
}

/// Parse a comma-separated address list, stripping whitespace and
/// skipping empty segments (so a trailing `,` is not an error).
///
/// Returns [`AddressParse::Empty`] if the input contains no
/// non-whitespace segment and [`AddressParse::InvalidAddress`] on
/// the first entry that does not parse as an EVM address. The
/// `index` reflects the zero-based position in the original
/// comma-separated list (i.e. it counts skipped empties), which
/// matches the wording the per-module errors used to surface.
pub fn parse_address_list(raw: &str) -> Result<Vec<Address>, AddressParse> {
    let mut out = Vec::new();
    for (i, part) in raw.split(',').enumerate() {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let addr = trimmed
            .parse::<Address>()
            .map_err(|e| AddressParse::InvalidAddress {
                index: i,
                raw: trimmed.to_owned(),
                message: e.to_string(),
            })?;
        out.push(addr);
    }
    if out.is_empty() {
        return Err(AddressParse::Empty);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn handles_whitespace_and_multiple() {
        let raw = "  0x70997970C51812dc3A010C7d01b50e0d17dc79C8 ,\
                   0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        let parsed = parse_address_list(raw).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            address!("70997970C51812dc3A010C7d01b50e0d17dc79C8"),
        );
    }

    #[test]
    fn skips_empty_segments() {
        let parsed = parse_address_list("0x70997970C51812dc3A010C7d01b50e0d17dc79C8,,").unwrap();
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn rejects_empty_list() {
        assert!(matches!(parse_address_list(""), Err(AddressParse::Empty)));
        assert!(matches!(
            parse_address_list(", ,"),
            Err(AddressParse::Empty)
        ));
    }

    #[test]
    fn rejects_malformed_entry() {
        match parse_address_list("not-an-address") {
            Err(AddressParse::InvalidAddress { index, raw, .. }) => {
                assert_eq!(index, 0);
                assert_eq!(raw, "not-an-address");
            }
            other => panic!("expected InvalidAddress, got {other:?}"),
        }
    }
}

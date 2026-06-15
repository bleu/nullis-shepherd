//! Small constructors that wrap the WIT `HostError` shape, used by
//! every `Host` trait impl, plus the lowercase hex encoder shared by
//! the `cow-api` submission path.

use crate::bindings::HostError;
use crate::bindings::nexum::host::types::HostErrorKind;

/// `Unsupported` (HTTP 501-style) error for capabilities the engine
/// reference build does not implement yet.
pub(crate) fn unimplemented(domain: &str, detail: impl Into<String>) -> HostError {
    HostError {
        domain: domain.into(),
        kind: HostErrorKind::Unsupported,
        code: 501,
        message: detail.into(),
        data: None,
    }
}

/// `Internal` (HTTP 500-style) error for unexpected backend failures.
pub(crate) fn internal_error(domain: &str, detail: impl Into<String>) -> HostError {
    HostError {
        domain: domain.into(),
        kind: HostErrorKind::Internal,
        code: 0,
        message: detail.into(),
        data: None,
    }
}

/// Lowercase hex encoder. Re-exports `alloy_primitives::hex::encode`
/// (already in the workspace dep graph for the chain backend) so the
/// engine does not roll its own per-byte `write!` loop.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    alloy_primitives::hex::encode(bytes)
}

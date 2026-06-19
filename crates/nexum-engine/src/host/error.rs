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

/// Lowercase hex encoder. Thin wrapper over
/// [`alloy_primitives::hex::encode`] so the engine reuses the
/// already-pulled alloy primitive instead of carrying its own
/// formatter (mfw78 review feedback on PR #8).
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    alloy_primitives::hex::encode(bytes)
}

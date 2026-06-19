//! Error types for manifest parsing and capability enforcement.

use super::types::KNOWN_CAPABILITIES;

/// Errors returned while loading or validating a manifest.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("manifest: i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest: parse: {0}")]
    Toml(#[from] toml::de::Error),
    #[error(
        "manifest: unknown capability {0:?} in [capabilities].required (known: {known})",
        known = KNOWN_CAPABILITIES.join(", ")
    )]
    UnknownCapability(String),
}

/// Error returned when a component's WIT imports exceed its declared capabilities.
#[derive(Debug, thiserror::Error)]
#[error(
    "component imports `{capability}` ({wit_import}) but it is not listed in \
     [capabilities].required or [capabilities].optional"
)]
pub struct CapabilityViolation {
    /// Capability name (e.g. `"remote-store"`).
    pub capability: String,
    /// Full WIT import name as it appeared in the component (e.g.
    /// `"nexum:host/remote-store@0.2.0"`).
    pub wit_import: String,
}

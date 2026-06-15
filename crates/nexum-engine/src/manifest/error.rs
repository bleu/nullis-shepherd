//! Error types for manifest parsing and capability enforcement.

use thiserror::Error;

use super::types::KNOWN_CAPABILITIES;

/// Errors returned while loading or validating a manifest.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Could not read the manifest file from disk.
    #[error("manifest: i/o: {0}")]
    Io(#[from] std::io::Error),
    /// The on-disk bytes did not parse as TOML.
    #[error("manifest: parse: {0}")]
    Toml(#[from] toml::de::Error),
    /// `[capabilities].required` named an interface the engine does
    /// not know how to provision. The display impl lists the known
    /// set so a typo surfaces with the corrected name in reach.
    #[error(
        "manifest: unknown capability {name:?} in [capabilities].required (known: {})",
        KNOWN_CAPABILITIES.join(", "),
    )]
    UnknownCapability {
        /// The unknown capability name as it appeared in the manifest.
        name: String,
    },
}

/// Error returned when a component's WIT imports exceed its declared capabilities.
#[derive(Debug, Error)]
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

//! Data structures: `Manifest`, sections, and `LoadedManifest`.
//!
//! Plain serde shapes plus the `KNOWN_CAPABILITIES` registry. The parsing
//! and validation logic lives in [`super::load`]; capability enforcement
//! in [`super::capabilities`].

use serde::Deserialize;

/// Capability names recognised by the 0.2 reference engine. Matches the
/// interfaces the `shepherd` world links into the linker.
pub const KNOWN_CAPABILITIES: &[&str] = &[
    "chain",
    "identity",
    "local-store",
    "remote-store",
    "messaging",
    "logging",
    "clock",
    "random",
    "http",
    // Domain-extension caps (provided by the shepherd world only):
    "cow-api",
];

#[derive(Debug, Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub module: ModuleSection,
    #[serde(default)]
    pub capabilities: Option<CapabilitiesSection>,
    #[serde(default)]
    pub config: toml::Table,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)] // version + component parsed for future 0.3 hash-verification.
pub struct ModuleSection {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub component: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct CapabilitiesSection {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
    #[serde(default)]
    pub http: Option<HttpSection>,
}

#[derive(Debug, Deserialize, Default)]
pub struct HttpSection {
    #[serde(default)]
    pub allow: Vec<String>,
}

/// Loaded + validated manifest, plus the data the engine needs to
/// instantiate a module.
pub struct LoadedManifest {
    pub manifest: Manifest,
    /// Hosts to allow for `http::fetch`. Each entry is either an exact
    /// hostname or a `*.suffix` wildcard.
    pub http_allowlist: Vec<String>,
    /// `[config]` flattened to `(key, stringified-value)` pairs ready to
    /// hand to a module's `init`. TOML scalars (string, integer, float,
    /// boolean) become their text form. Arrays and tables are rendered as
    /// their TOML representation.
    pub config: Vec<(String, String)>,
}

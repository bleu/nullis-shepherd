//! Parse `module.toml` from disk, validate, and emit operator-visible
//! warnings.
//!
//! Also exposes the small URL/host helpers the `http` host backend
//! uses to enforce the manifest's `[capabilities.http].allow` list at
//! request time.

use std::collections::HashSet;
use std::path::Path;

use super::error::ParseError;
use super::types::{KNOWN_CAPABILITIES, LoadedManifest, Manifest};

/// Read `module.toml` from `path`, parse, validate, and emit a deprecation
/// warning if `[capabilities]` is absent (0.1-compat fallback).
pub fn load(path: &Path) -> Result<LoadedManifest, ParseError> {
    let raw = std::fs::read_to_string(path).map_err(ParseError::Io)?;
    let manifest: Manifest = toml::from_str(&raw).map_err(ParseError::Toml)?;

    let caps = manifest.capabilities.as_ref();
    if caps.is_none() {
        eprintln!(
            "[deprecation] no [capabilities] section in module.toml - \
             defaulting to all-required (0.1 behaviour). This default \
             will be removed in 0.3; add an explicit [capabilities] block."
        );
    }

    if let Some(c) = caps {
        let known: HashSet<&str> = KNOWN_CAPABILITIES.iter().copied().collect();
        for name in c.required.iter().chain(c.optional.iter()) {
            if !known.contains(name.as_str()) {
                return Err(ParseError::UnknownCapability(name.clone()));
            }
        }
        if !c.required.is_empty() {
            eprintln!(
                "[manifest] required capabilities: {}",
                c.required.join(", ")
            );
        }
        if !c.optional.is_empty() {
            eprintln!(
                "[manifest] optional capabilities (advisory in 0.2; trap-stub fallback \
                 ships in 0.3): {}",
                c.optional.join(", ")
            );
        }
    }

    let http_allowlist = caps
        .and_then(|c| c.http.as_ref())
        .map(|h| h.allow.clone())
        .unwrap_or_default();
    if !http_allowlist.is_empty() {
        eprintln!("[manifest] http allowlist: {}", http_allowlist.join(", "));
    }

    let config = manifest
        .config
        .iter()
        .map(|(k, v)| (k.clone(), stringify_toml_value(v)))
        .collect();

    Ok(LoadedManifest {
        manifest,
        http_allowlist,
        config,
    })
}

/// Synthesise a "0.1 fallback" manifest for when no `module.toml` is found.
/// Emits the same deprecation warning as a missing-section manifest.
pub fn fallback_manifest() -> LoadedManifest {
    eprintln!(
        "[deprecation] no module.toml found - defaulting to all-required \
         (0.1 behaviour). This default will be removed in 0.3; ship a \
         module.toml alongside your component."
    );
    LoadedManifest {
        manifest: Manifest::default(),
        http_allowlist: Vec::new(),
        config: Vec::new(),
    }
}

/// Check whether `host` matches any pattern in the allowlist. Patterns are
/// either exact (`api.example.com`) or `*.suffix` wildcards which match
/// any subdomain of `suffix` (but not `suffix` itself).
pub fn host_allowed(host: &str, allowlist: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    allowlist.iter().any(|pat| {
        let pat = pat.to_ascii_lowercase();
        if let Some(suffix) = pat.strip_prefix("*.") {
            host.ends_with(&format!(".{suffix}"))
        } else {
            host == pat
        }
    })
}

/// Extract the host component from a URL. Returns `None` for non-http(s)
/// schemes or malformed input. Intentionally simple - adds no `url`
/// crate dependency.
pub fn extract_host(url: &str) -> Option<&str> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host_end = after_scheme
        .find('/')
        .or_else(|| after_scheme.find('?'))
        .unwrap_or(after_scheme.len());
    let host = &after_scheme[..host_end];
    // strip optional user-info and port.
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() { None } else { Some(host) }
}

fn stringify_toml_value(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(_) | toml::Value::Table(_) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_host_handles_common_shapes() {
        assert_eq!(
            extract_host("https://api.example.com/v1/x"),
            Some("api.example.com")
        );
        assert_eq!(extract_host("http://example.com"), Some("example.com"));
        assert_eq!(
            extract_host("https://user:pw@host.example.com:8443/x"),
            Some("host.example.com")
        );
        assert_eq!(extract_host("https://example.com?q=1"), Some("example.com"));
        assert_eq!(extract_host("ftp://example.com"), None);
        assert_eq!(extract_host("not a url"), None);
    }

    #[test]
    fn host_allowed_exact_and_wildcard() {
        let allow = vec!["api.cow.fi".to_string(), "*.discord.com".to_string()];
        assert!(host_allowed("api.cow.fi", &allow));
        assert!(!host_allowed("evil.api.cow.fi", &allow));
        assert!(host_allowed("foo.discord.com", &allow));
        assert!(host_allowed("a.b.discord.com", &allow));
        assert!(!host_allowed("discord.com", &allow));
        assert!(!host_allowed("nope.example", &allow));
    }
}

//! CLI surface, parsed via `clap`'s derive API.

use std::path::PathBuf;

use clap::Parser;

/// `nexum-engine` argument parser.
///
/// Production deployments pass `--engine-config <path>` and declare
/// modules in TOML. The positional `<wasm-path>` (with optional
/// `<manifest-path>`) is a backwards-compat shortcut that synthesises
/// a one-module engine config so the historical
/// `cargo run -- ./modules/example/example.wasm` flow keeps working.
#[derive(Debug, Parser)]
#[command(
    name = "nexum-engine",
    about = "Multi-module supervisor for shepherd WASM components."
)]
pub struct Cli {
    /// Positional WASM component to boot when `--engine-config` is
    /// not supplied (or when its `[[modules]]` list is empty).
    pub wasm: Option<PathBuf>,

    /// Optional manifest (`module.toml`) sibling for the positional
    /// `<wasm-path>`. Ignored when `--engine-config` is supplied.
    pub manifest: Option<PathBuf>,

    /// Path to the `engine.toml` describing chains + modules. When
    /// omitted, falls back to the in-repo default (single-module mode).
    #[arg(long, value_name = "PATH")]
    pub engine_config: Option<PathBuf>,
}

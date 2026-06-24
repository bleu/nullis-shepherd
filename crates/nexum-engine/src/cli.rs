//! Manual CLI parser. Kept hand-rolled (instead of pulling clap) because
//! the surface is small and unlikely to grow in 0.2.

use std::path::PathBuf;

/// Parsed CLI surface.
///
/// `nexum-engine [<wasm-path> [<manifest-path>]] [--engine-config <path>]`
///
/// Positional `<wasm-path>` is a backwards-compat shortcut that
/// synthesises a one-module engine config. Production deployments pass
/// `--engine-config` and declare modules in TOML.
#[derive(Debug, Default)]
pub struct Cli {
    pub wasm: Option<PathBuf>,
    pub manifest: Option<PathBuf>,
    pub engine_config: Option<PathBuf>,
}

impl Cli {
    pub fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut cli = Self::default();
        let mut positional = Vec::new();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--engine-config" => cli.engine_config = args.next().map(PathBuf::from),
                "-h" | "--help" => {
                    eprintln!(
                        "usage: nexum-engine [<wasm-path> [<manifest-path>]] \
                         [--engine-config <path>]"
                    );
                    std::process::exit(0);
                }
                _ => positional.push(arg),
            }
        }
        if let Some(p) = positional.first() {
            cli.wasm = Some(PathBuf::from(p));
        }
        if let Some(p) = positional.get(1) {
            cli.manifest = Some(PathBuf::from(p));
        }
        cli
    }
}

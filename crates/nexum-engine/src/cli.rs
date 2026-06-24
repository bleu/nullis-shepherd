//! Manual CLI parser. Kept hand-rolled (instead of pulling clap) because
//! the surface is small and unlikely to grow in 0.2.

use std::path::PathBuf;

/// Parsed CLI surface.
///
/// `nexum-engine [<wasm-path> [<manifest-path>]] [--engine-config <path>] [--pretty-logs]`
///
/// Positional `<wasm-path>` is a backwards-compat shortcut that
/// synthesises a one-module engine config. Production deployments pass
/// `--engine-config` and declare modules in TOML.
///
/// `--pretty-logs` selects the human-readable tracing formatter (the
/// historical 0.1 default). Without the flag the engine emits JSON
/// log lines per the COW-1035 structured-logging contract: a single
/// `jq` / Loki / Grafana stream reconstructs the full timeline of
/// any dispatch, host call, or order submission.
#[derive(Debug, Default)]
pub struct Cli {
    pub wasm: Option<PathBuf>,
    pub manifest: Option<PathBuf>,
    pub engine_config: Option<PathBuf>,
    pub pretty_logs: bool,
}

impl Cli {
    pub fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut cli = Self::default();
        let mut positional = Vec::new();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--engine-config" => cli.engine_config = args.next().map(PathBuf::from),
                "--pretty-logs" => cli.pretty_logs = true,
                "-h" | "--help" => {
                    eprintln!(
                        "usage: nexum-engine [<wasm-path> [<manifest-path>]] \
                         [--engine-config <path>] [--pretty-logs]"
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

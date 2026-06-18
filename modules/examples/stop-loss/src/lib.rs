//! # stop-loss (example Shepherd module)
//!
//! Watches a Chainlink price oracle on every block. When the price
//! drops at or below `trigger_price`, the module submits a pre-signed
//! CoW order using the parameters from `module.toml::[config]` and
//! persists `submitted:{uid}` to dedup re-poll attempts. The owner is
//! expected to have called `GPv2Signing.setPreSignature` on-chain
//! ahead of the trigger so the orderbook accepts the submission.
//!
//! ## Module layout
//!
//! - `strategy.rs` holds the pure logic and tests against
//!   `shepherd_sdk::host::Host`. It does not know `wit-bindgen`
//!   exists.
//! - `lib.rs` (this file) is the per-cdylib glue: wit-bindgen import
//!   shims, the `WitBindgenHost` adapter, the `Guest` impl.
//!
//! Same recipe as `price-alert` (BLEU-851) - the wit-bindgen adapter
//! is intentionally mechanical and is a candidate for a future
//! declarative macro in `shepherd-sdk`.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![allow(clippy::too_many_arguments)]

wit_bindgen::generate!({
    path: ["../../../wit/nexum-host", "../../../wit/shepherd-cow"],
    world: "shepherd:cow/shepherd",
    generate_all,
});

mod strategy;

use std::sync::OnceLock;

use shepherd_sdk::host::{
    ChainHost, CowApiHost, HostError as SdkHostError, HostErrorKind as SdkHostErrorKind,
    LocalStoreHost, LogLevel as SdkLogLevel, LoggingHost,
};

use nexum::host::types::HostErrorKind;
use nexum::host::{chain, local_store, logging, types};
use shepherd::cow::cow_api;

static SETTINGS: OnceLock<strategy::Settings> = OnceLock::new();

struct WitBindgenHost;

impl ChainHost for WitBindgenHost {
    fn request(&self, chain_id: u64, method: &str, params: &str) -> Result<String, SdkHostError> {
        chain::request(chain_id, method, params).map_err(convert_err)
    }
}

impl LocalStoreHost for WitBindgenHost {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SdkHostError> {
        local_store::get(key).map_err(convert_err)
    }
    fn set(&self, key: &str, value: &[u8]) -> Result<(), SdkHostError> {
        local_store::set(key, value).map_err(convert_err)
    }
    fn delete(&self, key: &str) -> Result<(), SdkHostError> {
        local_store::delete(key).map_err(convert_err)
    }
    fn list_keys(&self, prefix: &str) -> Result<Vec<String>, SdkHostError> {
        local_store::list_keys(prefix).map_err(convert_err)
    }
}

impl CowApiHost for WitBindgenHost {
    fn submit_order(&self, chain_id: u64, body: &[u8]) -> Result<String, SdkHostError> {
        cow_api::submit_order(chain_id, body).map_err(convert_err)
    }
}

impl LoggingHost for WitBindgenHost {
    fn log(&self, level: SdkLogLevel, message: &str) {
        logging::log(convert_level(level), message);
    }
}

fn convert_err(e: HostError) -> SdkHostError {
    SdkHostError {
        domain: e.domain,
        kind: match e.kind {
            HostErrorKind::Unsupported => SdkHostErrorKind::Unsupported,
            HostErrorKind::Unavailable => SdkHostErrorKind::Unavailable,
            HostErrorKind::Denied => SdkHostErrorKind::Denied,
            HostErrorKind::RateLimited => SdkHostErrorKind::RateLimited,
            HostErrorKind::Timeout => SdkHostErrorKind::Timeout,
            HostErrorKind::InvalidInput => SdkHostErrorKind::InvalidInput,
            HostErrorKind::Internal => SdkHostErrorKind::Internal,
        },
        code: e.code,
        message: e.message,
        data: e.data,
    }
}

fn sdk_err_into_wit(e: SdkHostError) -> HostError {
    HostError {
        domain: e.domain,
        kind: match e.kind {
            SdkHostErrorKind::Unsupported => HostErrorKind::Unsupported,
            SdkHostErrorKind::Unavailable => HostErrorKind::Unavailable,
            SdkHostErrorKind::Denied => HostErrorKind::Denied,
            SdkHostErrorKind::RateLimited => HostErrorKind::RateLimited,
            SdkHostErrorKind::Timeout => HostErrorKind::Timeout,
            SdkHostErrorKind::InvalidInput => HostErrorKind::InvalidInput,
            SdkHostErrorKind::Internal => HostErrorKind::Internal,
            // Wildcard: `SdkHostErrorKind` is `#[non_exhaustive]` so the SDK
            // can grow new variants without breaking module adapters. Fall back
            // to `Internal` as the safest catch-all (COW-1029).
            _ => HostErrorKind::Internal,
        },
        code: e.code,
        message: e.message,
        data: e.data,
    }
}

fn convert_level(l: SdkLogLevel) -> logging::Level {
    match l {
        SdkLogLevel::Trace => logging::Level::Trace,
        SdkLogLevel::Debug => logging::Level::Debug,
        SdkLogLevel::Info => logging::Level::Info,
        SdkLogLevel::Warn => logging::Level::Warn,
        SdkLogLevel::Error => logging::Level::Error,
        // Wildcard: `SdkLogLevel` is `#[non_exhaustive]` (COW-1029).
        // Fall back to `Info` for any future SDK-side variant.
        _ => logging::Level::Info,
    }
}

struct StopLoss;

impl Guest for StopLoss {
    fn init(config: Vec<(String, String)>) -> Result<(), HostError> {
        let cfg = strategy::parse_config(&config).map_err(sdk_err_into_wit)?;
        logging::log(
            logging::Level::Info,
            &format!(
                "stop-loss init: owner={:#x} trigger={} sell={:#x} buy={:#x}",
                cfg.owner, cfg.trigger_price_scaled, cfg.sell_token, cfg.buy_token,
            ),
        );
        let _ = SETTINGS.set(cfg);
        Ok(())
    }

    fn on_event(event: types::Event) -> Result<(), HostError> {
        let Some(cfg) = SETTINGS.get() else {
            return Ok(());
        };
        if let types::Event::Block(block) = event {
            strategy::on_block(&WitBindgenHost, block.chain_id, cfg).map_err(sdk_err_into_wit)?;
        }
        Ok(())
    }
}

export!(StopLoss);

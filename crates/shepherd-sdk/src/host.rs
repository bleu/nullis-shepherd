//! Host traits - the seam between strategy logic and the wit-bindgen
//! shims a module generates per-cdylib.
//!
//! Each trait mirrors one nexum / shepherd host interface
//! ([`ChainHost`] for `nexum:host/chain`, [`LocalStoreHost`] for
//! `nexum:host/local-store`, [`CowApiHost`] for `shepherd:cow/cow-api`,
//! [`LoggingHost`] for `nexum:host/logging`). A module that wants
//! host-free unit tests writes its strategy logic against the
//! [`Host`] supertrait and lets `shepherd-sdk-test` slot in the
//! in-memory mocks.
//!
//! ## Why a separate `HostError`
//!
//! `wit_bindgen::generate!` emits a `HostError` struct into each
//! module's own crate, so its identity is per-module. The SDK
//! exposes [`HostError`] (this module) with the same field shape  -
//! modules wire a one-liner `From` impl between the two so the
//! traits stay world-neutral and the mocks compile without a wasm
//! toolchain. See `shepherd-sdk-test`'s README for the adapter
//! pattern.

/// Severity for log messages routed through [`LoggingHost::log`].
/// Mirrors `nexum:host/logging.level`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LogLevel {
    /// Verbose tracing for development.
    Trace,
    /// Detail useful to operators when investigating.
    Debug,
    /// Steady-state events.
    Info,
    /// Recoverable errors - operator notice but no immediate action.
    Warn,
    /// Unrecoverable errors - operator should investigate.
    Error,
}

/// Coarse categorisation of host failures, mirrored verbatim from
/// `nexum:host/types.host-error-kind` so a module's wit-bindgen
/// `HostErrorKind` can convert one-to-one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum HostErrorKind {
    /// Capability declared but not provisioned by the operator.
    Unsupported,
    /// Capability temporarily unavailable (RPC down, etc).
    Unavailable,
    /// Capability declined the request (auth, allowlist, …).
    Denied,
    /// Rate-limited by an upstream service.
    RateLimited,
    /// Operation took too long.
    Timeout,
    /// Caller-supplied input did not parse / validate.
    InvalidInput,
    /// Catch-all for host-side bugs.
    Internal,
}

/// SDK-side counterpart to wit-bindgen's `HostError`. Same field shape
/// so a module bridges between the two with a trivial `From` impl on
/// each side.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{domain}: {message} (code={code}, kind={kind:?})")]
pub struct HostError {
    /// Short subsystem identifier (`"chain"`, `"local-store"`,
    /// `"cow-api"`, `"logging"`).
    pub domain: String,
    /// See [`HostErrorKind`].
    pub kind: HostErrorKind,
    /// Domain-specific numeric (HTTP status, JSON-RPC code, etc).
    pub code: i32,
    /// Human-readable detail.
    pub message: String,
    /// Optional opaque payload (often JSON-encoded).
    pub data: Option<String>,
}

impl HostError {
    /// Convenience constructor for unsupported / not-yet-implemented
    /// host endpoints. Useful in tests and mock setups.
    pub fn unsupported(domain: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            kind: HostErrorKind::Unsupported,
            code: 501,
            message: message.into(),
            data: None,
        }
    }
}

/// `nexum:host/chain` - raw JSON-RPC dispatch.
pub trait ChainHost {
    /// Execute a JSON-RPC request against the given chain. The host
    /// routes to its configured provider; the SDK does not care which
    /// transport (HTTP / WebSocket / mock) implements the call.
    fn request(&self, chain_id: u64, method: &str, params: &str) -> Result<String, HostError>;
}

/// `nexum:host/local-store` - per-module key-value persistence.
pub trait LocalStoreHost {
    /// Fetch a value. `Ok(None)` when the key is absent.
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostError>;
    /// Insert or overwrite.
    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostError>;
    /// Delete. No-op if the key is absent.
    fn delete(&self, key: &str) -> Result<(), HostError>;
    /// Enumerate keys whose raw form starts with `prefix`.
    fn list_keys(&self, prefix: &str) -> Result<Vec<String>, HostError>;
}

/// `shepherd:cow/cow-api` - orderbook submission path.
pub trait CowApiHost {
    /// Submit an `OrderCreation` JSON body. The host returns the
    /// canonical order UID on success.
    fn submit_order(&self, chain_id: u64, body: &[u8]) -> Result<String, HostError>;
}

/// `nexum:host/logging` - structured runtime logs.
pub trait LoggingHost {
    /// Emit a log line at the given level.
    fn log(&self, level: LogLevel, message: &str);
}

/// Supertrait that bundles the four host interfaces a typical strategy
/// module exercises. Modules that want full host-free integration
/// tests take `&impl Host` (or a generic `<H: Host>`) in their
/// strategy function; `shepherd-sdk-test::MockHost` is the in-memory
/// implementation.
///
/// A blanket impl is provided for any type that implements all four
/// component traits, so callers do not have to add a redundant
/// `impl Host for MyHost {}`.
///
/// # Example
///
/// Strategy functions are generic over [`Host`]. Production code plugs
/// the per-module `WitBindgenHost` adapter (see `modules/examples/`);
/// unit tests plug `shepherd_sdk_test::MockHost`.
///
/// ```
/// use shepherd_sdk::host::{
///     ChainHost, CowApiHost, Host, HostError, LocalStoreHost, LogLevel, LoggingHost,
/// };
///
/// /// Pure strategy logic - no wit-bindgen calls in here.
/// fn record_block<H: Host>(host: &H, chain_id: u64, key: &str) -> Result<(), HostError> {
///     host.log(LogLevel::Info, "recording block");
///     host.set(key, b"")?;
///     let _block_number = host.request(chain_id, "eth_blockNumber", "[]")?;
///     Ok(())
/// }
///
/// // Minimal hand-rolled host so the doctest is self-contained.
/// // Real modules wire `shepherd_sdk_test::MockHost` here.
/// # struct StubHost;
/// # impl ChainHost for StubHost {
/// #     fn request(&self, _: u64, _: &str, _: &str) -> Result<String, HostError> {
/// #         Ok("\"0x0\"".into())
/// #     }
/// # }
/// # impl LocalStoreHost for StubHost {
/// #     fn get(&self, _: &str) -> Result<Option<Vec<u8>>, HostError> { Ok(None) }
/// #     fn set(&self, _: &str, _: &[u8]) -> Result<(), HostError> { Ok(()) }
/// #     fn delete(&self, _: &str) -> Result<(), HostError> { Ok(()) }
/// #     fn list_keys(&self, _: &str) -> Result<Vec<String>, HostError> { Ok(vec![]) }
/// # }
/// # impl CowApiHost for StubHost {
/// #     fn submit_order(&self, _: u64, _: &[u8]) -> Result<String, HostError> { Ok("".into()) }
/// # }
/// # impl LoggingHost for StubHost {
/// #     fn log(&self, _: LogLevel, _: &str) {}
/// # }
/// record_block(&StubHost, 1, "block:42").unwrap();
/// ```
pub trait Host: ChainHost + LocalStoreHost + CowApiHost + LoggingHost {}
impl<T: ChainHost + LocalStoreHost + CowApiHost + LoggingHost> Host for T {}

# Build your first Shepherd module

This is the cold-start guide for an external developer. Target
completion time: **under four hours** from "I cloned the repo" to
"I see my module's first event in the engine log".

The walked-through example is **stop-loss**: a module that watches a
Chainlink price oracle on every block and submits a pre-signed CoW
order when the price drops below a configured trigger. The fully
working source lives at [`modules/examples/stop-loss/`](
../modules/examples/stop-loss). The rest of this guide reads that
source top-to-bottom and explains *why* each piece is shaped the
way it is. Open the files alongside the guide as you read.

## 0. Prerequisites (15 minutes)

You need a recent Rust toolchain (`rustc 1.91+`, ships with `cargo`)
and the WASM Component Model target. From the repo root:

```sh
rustup target add wasm32-wasip2
```

Verify the engine builds and runs against the example module that
ships in the workspace:

```sh
cargo build --target wasm32-wasip2 --release -p example
cargo run -p nexum-engine -- \
  target/wasm32-wasip2/release/example.wasm \
  modules/example/nexum.toml
```

You should see two log lines from the example module - one in
`init`, one on the synthetic block event. Stop here and triage if
the build fails or those log lines do not appear; the rest of the
tutorial assumes a working local engine.

Now build the stop-loss module:

```sh
cargo build --target wasm32-wasip2 --release -p stop-loss
ls -lh target/wasm32-wasip2/release/stop_loss.wasm
```

Expected size: ~300 KB.

## 1. Anatomy of a module (10 minutes)

A Shepherd module is a Cargo crate with `crate-type = ["cdylib"]`
compiled to `wasm32-wasip2`. The minimum layout:

```
modules/examples/stop-loss/
├── Cargo.toml          declares deps (shepherd-sdk, cowprotocol, alloy, ...)
├── module.toml         declares capabilities + subscriptions + config
└── src/
    ├── lib.rs          wit-bindgen glue + Guest impl + adapter
    └── strategy.rs     pure logic against `shepherd_sdk::host::Host`
```

The split into `lib.rs` (impure / wit-bindgen) and `strategy.rs`
(pure / `&impl Host`) is the recipe that lets you test the strategy
end-to-end against `shepherd-sdk-test::MockHost` without ever
running the wasm toolchain.

Open [`Cargo.toml`](../modules/examples/stop-loss/Cargo.toml) and
note the four key features:

- **`crate-type = ["cdylib"]`** - produces a WASM Component when
  built for `wasm32-wasip2`.
- **`shepherd-sdk` path dep** - the helpers (`cow::`, `chain::`,
  `host::`, `prelude`) live here.
- **`shepherd-sdk-test` as a dev-dep** - `MockHost` is only linked
  under `cargo test`, never in the wasm bundle.
- **No `nexum-engine` dep** - modules never link the engine; they
  communicate exclusively through wit-bindgen-generated shims.

The workspace `Cargo.toml` at the repo root has the crate listed
under `[workspace] members`.

## 2. The manifest: capabilities and config (10 minutes)

Open [`module.toml`](../modules/examples/stop-loss/module.toml).
Two things matter:

```toml
[capabilities]
required = ["logging", "chain", "local-store", "cow-api"]
```

The engine enforces this list against the WIT imports the
compiled component declares. Declaring a capability you do not
use is fine; *missing* one you do use is a hard error at
instantiation. Stop-loss touches all four:

| Capability | Used for |
|---|---|
| `logging` | every Info / Warn line |
| `chain` | the `eth_call` to read the oracle |
| `local-store` | the `submitted:{uid}` and `dropped:{uid}` dedup flags |
| `cow-api` | submitting the `OrderCreation` body |

```toml
[[subscription]]
kind = "block"
chain_id = 11155111  # Sepolia
```

Stop-loss reacts to every new block on Sepolia. WebSocket RPC is
required because `block` rides `eth_subscribe`; see
[`docs/deployment.md`](./deployment.md) for the operator-side
chain config.

```toml
[config]
oracle_address = "0x694AA1..."
decimals = "8"
trigger_price = "2500.00"
owner = "0x70997970..."
sell_token = "..."
buy_token = "..."
sell_amount_wei = "..."
buy_amount_wei = "..."
valid_to_seconds = "..."
```

`[config]` is operator-supplied. The values are strings; the
module parses them once at `init`. We will look at the parsing
code in §4.

## 3. The wit-bindgen adapter in `lib.rs` (15 minutes)

Open [`src/lib.rs`](../modules/examples/stop-loss/src/lib.rs). Top
of the file:

```rust
wit_bindgen::generate!({
    path: ["../../../wit/nexum-host", "../../../wit/shepherd-cow"],
    world: "shepherd:cow/shepherd",
    generate_all,
});

mod strategy;
```

The `generate!` macro emits the per-cdylib `Guest` trait,
`HostError` struct, and host import shims (`nexum::host::chain::
request`, `local_store::set`, etc.) into this crate's scope.
`generate_all` is required because the `shepherd:cow/shepherd`
world cross-references types from `nexum:host/types` - see
[`docs/sdk.md`](./sdk.md) for the gotcha.

Below the macro, three blocks deserve attention:

### 3a. `WitBindgenHost` (~80 lines)

```rust
struct WitBindgenHost;

impl ChainHost for WitBindgenHost {
    fn request(&self, chain_id: u64, method: &str, params: &str)
        -> Result<String, SdkHostError>
    {
        chain::request(chain_id, method, params).map_err(convert_err)
    }
}
// ... LocalStoreHost / CowApiHost / LoggingHost ...
```

This is the bridge between wit-bindgen's free functions and the
`shepherd_sdk::host::Host` trait the strategy works against. The
shape is mechanical and identical across modules - copy it as-is
into your own module, and a future declarative macro in
`shepherd-sdk` will eventually elide it.

### 3b. `convert_err` / `sdk_err_into_wit` / `convert_level`

`wit_bindgen::generate!` emits a `HostError` struct into the
module's own crate. `shepherd_sdk::host::HostError` is a *separate*
type with the same fields. The three converters are 7-arm enum
maps - mechanical, but necessary so the trait surface can stay
world-neutral.

### 3c. `Guest for StopLoss`

```rust
impl Guest for StopLoss {
    fn init(config: Vec<(String, String)>) -> Result<(), HostError> {
        let cfg = strategy::parse_config(&config).map_err(sdk_err_into_wit)?;
        // ... log + cache in OnceLock ...
    }

    fn on_event(event: types::Event) -> Result<(), HostError> {
        let Some(cfg) = SETTINGS.get() else { return Ok(()); };
        if let types::Event::Block(block) = event {
            strategy::on_block(&WitBindgenHost, block.chain_id, cfg)
                .map_err(sdk_err_into_wit)?;
        }
        Ok(())
    }
}
```

`init` parses + caches; `on_event` hands a `WitBindgenHost` to the
strategy and translates the resulting `SdkHostError` back into the
wit-bindgen one for the supervisor.

`SETTINGS: OnceLock<strategy::Settings>` is the recommended
single-init pattern. wasm32 modules are single-threaded so
`OnceLock` is overkill on synchronisation but cheap and explicit
about lifetime.

## 4. The strategy in `strategy.rs` (45 minutes)

Open [`src/strategy.rs`](../modules/examples/stop-loss/src/strategy.rs).
This file is the heart of the module - the only one you would
diff against if you rebased on a newer SDK.

### 4a. `Settings` + `parse_config`

The parser walks `Vec<(String, String)>` and produces a typed
`Settings`. It returns `Result<Settings, shepherd_sdk::host::
HostError>` so the upstream `Guest::init` can lift the failure
straight into the wit-bindgen `HostError` envelope with no extra
plumbing. `scale_signed` is a hand-rolled decimal-to-I256 scaler
because alloy ships no `Decimal::parse_units` equivalent (yet).

### 4b. `read_oracle`

```rust
fn read_oracle<H: Host>(host: &H, chain_id: u64, oracle: Address)
    -> Option<I256>
{
    let call_data = AggregatorV3::latestRoundDataCall {}.abi_encode();
    let params = eth_call_params(&oracle, &call_data);
    let result_json = host.request(chain_id, "eth_call", &params).ok()?;
    let bytes = parse_eth_call_result(&result_json)?;
    AggregatorV3::latestRoundDataCall::abi_decode_returns(&bytes)
        .ok()
        .map(|r| r.answer)
}
```

Three SDK helpers in three lines: `chain::eth_call_params` builds
the `[{to, data}, "latest"]` JSON, `chain::parse_eth_call_result`
unpacks the `"0x..."` hex response. The `sol! interface AggregatorV3`
declared at the top of the file gives us a typed call + return
decoder; the same pattern works for any read-only EVM contract.

Returning `Option<I256>` (with a Warn log on the error path inside
the function) is intentional: the next block re-polls, and a
single flaky RPC reply should not propagate into the supervisor.

### 4c. `build_creation`

The most interesting piece. Constructs a `cowprotocol::
OrderCreation` body the orderbook accepts:

```rust
let chain = Chain::try_from(chain_id)?;
let domain = chain.settlement_domain();
let gpv2 = GPv2OrderData { ... };
let order_data = gpv2_to_order_data(&gpv2)?;  // shepherd-sdk helper
let uid = order_data.uid(&domain, settings.owner);
let creation = OrderCreation::from_signed_order_data(
    &order_data,
    Signature::PreSign,    // owner has called setPreSignature on-chain
    settings.owner,
    EMPTY_APP_DATA_JSON.to_string(),
    None,
)?;
```

Three load-bearing decisions:

- **`Signature::PreSign`**: the module ships no ECDSA. The order
  owner is expected to have called `GPv2Signing.setPreSignature`
  on-chain ahead of the trigger. The body shipped to the orderbook
  carries the owner address and an empty signature; the orderbook
  validates by checking the on-chain pre-signature record at
  settlement.
- **`gpv2_to_order_data`**: the `shepherd-sdk` helper that maps the
  on-chain `bytes32` markers (`kind`, balance sources) onto
  cowprotocol's typed enums. Same code-path twap-monitor and
  ethflow-watcher take after the BLEU-843 refactor.
- **`order_data.uid(&domain, settings.owner)`**: computes the
  canonical 56-byte UID locally. The orderbook's `POST /api/v1/
  orders` returns the same UID; the module uses the local version
  to dedup *before* paying for the network round-trip.

### 4d. `on_block`

The dispatch loop:

```rust
pub fn on_block<H: Host>(host: &H, chain_id: u64, settings: &Settings)
    -> Result<(), HostError>
{
    let price = read_oracle(host, chain_id, settings.oracle_address) else { return Ok(()) };

    if price > settings.trigger_price_scaled {
        // idle - log and wait for the next block
        return Ok(());
    }

    let (creation, uid) = build_creation(chain_id, settings)?;
    let uid_hex = format!("{uid}");

    // Dedup: skip if already submitted OR previously dropped.
    if host.get(&format!("submitted:{uid_hex}"))?.is_some() { return Ok(()); }
    if host.get(&format!("dropped:{uid_hex}"))?.is_some()   { return Ok(()); }

    let body = serde_json::to_vec(&creation)?;
    match host.submit_order(chain_id, &body) {
        Ok(server_uid) => {
            host.set(&format!("submitted:{server_uid}"), b"")?;
            host.log(LogLevel::Warn, &format!("TRIGGERED, uid={server_uid}"));
        }
        Err(err) => match classify_api_error(err.data.as_deref()) {
            RetryAction::TryNextBlock | RetryAction::Backoff { .. } => {
                // log and let the next block re-attempt
            }
            RetryAction::Drop => {
                host.set(&format!("dropped:{uid_hex}"), b"")?;
                // log + give up - the orderbook will not accept the
                // same body on a retry
            }
        },
    }
    Ok(())
}
```

The `shepherd_sdk::cow::classify_api_error` helper is the BLEU-829
retry contract - it maps the orderbook's typed `ApiError` into
`TryNextBlock` / `Backoff` / `Drop`. The module's only role here is
to act on the verdict: log and idle, or persist a `dropped:` flag
so the next block does not re-attempt.

### 4e. Tests at the bottom

Seven tests cover the dispatch matrix:

- `idle_when_price_above_trigger`
- `triggers_and_submits_once_then_dedups`
- `permanent_submit_error_marks_dropped` (+ confirms dedup on the
  next block)
- `transient_submit_error_leaves_state_unchanged`
- `oracle_rpc_error_is_warn_and_continue`
- `parse_config_round_trips_settings` + `parse_config_rejects_
  missing_owner`

All seven run against `shepherd_sdk_test::MockHost`. `host.chain.
respond_to(...)` programs the oracle return; `host.cow_api.respond
(...)` programs the orderbook response; assertions read
`host.store.snapshot()` and `host.logging.contains(...)`. No
`wasmtime`, no network, no fixture wasm bundle.

## 5. Build the `.wasm` (5 minutes)

You already did this in §0. Re-build to confirm the strategy edits
compile:

```sh
cargo build --target wasm32-wasip2 --release -p stop-loss
ls -lh target/wasm32-wasip2/release/stop_loss.wasm
```

If the file ballooned past ~500 KB, look at
`cargo tree -p stop-loss --target wasm32-wasip2` - usually a fresh
dependency pulled `reqwest` or `tokio` into the wasm graph.

## 6. Wire `engine.toml` and run it (10 minutes)

Add a Sepolia RPC entry:

```toml
[chains.11155111]
rpc_url = "wss://ethereum-sepolia-rpc.publicnode.com"
```

WebSocket is required because the `[[subscription]]` is `kind =
"block"`. Run:

```sh
cargo run -p nexum-engine -- \
  target/wasm32-wasip2/release/stop_loss.wasm \
  modules/examples/stop-loss/module.toml
```

Expected output:

- `init`: `stop-loss init: owner=0x... trigger=...`
- on each new block: `stop-loss idle: price=... > trigger=...`
  while the oracle stays above the threshold, then `stop-loss
  TRIGGERED: ...` if the price ever drops at or below.

If the engine reports `unsupported` for any capability, double-
check `[capabilities].required` matches the imports the strategy
exercises.

For multi-module operation (running stop-loss alongside other
strategies), see the BLEU-818 supervisor PR.

## 7. Where to go from here (10 minutes)

- **Production hardening**: tune `[engine.limits].fuel_per_event`
  and `memory_bytes` for your hardware - see [`docs/deployment.md`](
  ./deployment.md) for the operator runbook.
- **A different strategy**: copy `modules/examples/stop-loss/`,
  rename, and change `on_block`. The wit-bindgen adapter in
  `lib.rs` is identical for every module; only `strategy.rs` and
  `module.toml::[config]` move.
- **Custom signing**: swap `Signature::PreSign` for
  `Signature::Eip1271(bytes)` when the owner is a Safe with an
  isValidSignature handler - same pattern ethflow-watcher uses.
- **Multi-chain operation**: change `[[subscription]].chain_id`
  and add the `engine.toml::[chains.<id>]` entry. The strategy
  stays unchanged because every host call passes `chain_id`
  through.

## Time-budget check

If a section ran much longer than the rough estimate above, please
file an issue tagged `docs/tutorial` with the section that dragged.
The target is **<4h cold from a fresh checkout to a successful run
in §6**, and we tighten the prose against feedback.

## Reference index

- SDK overview: [`docs/sdk.md`](./sdk.md)
- Deployment runbook: [`docs/deployment.md`](./deployment.md)
- The example: [`modules/examples/stop-loss/`](
  ../modules/examples/stop-loss/)
- ADR-0001 (`engine.toml` vs `module.toml` split)
- ADR-0006 (TWAP / EthFlow as guest modules, no specialised
  WIT interfaces)
- ADR-0007 (push protocol primitives to `cow-rs` first)
- Worked examples that share the same recipe:
  [`price-alert`](../modules/examples/price-alert/),
  [`balance-tracker`](../modules/examples/balance-tracker/),
  [`twap-monitor`](../modules/twap-monitor/),
  [`ethflow-watcher`](../modules/ethflow-watcher/)

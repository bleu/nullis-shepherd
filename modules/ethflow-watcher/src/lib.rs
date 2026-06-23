// wit_bindgen::generate! expands to host-import shims whose arity matches
// the WIT signatures, which can exceed clippy's too-many-arguments threshold.
#![allow(clippy::too_many_arguments)]

wit_bindgen::generate!({
    path: ["../../wit/nexum-host", "../../wit/shepherd-cow"],
    world: "shepherd:cow/shepherd",
    generate_all,
});

use alloy_primitives::{Address, B256, Bytes};
use alloy_sol_types::SolEvent;
use cowprotocol::{
    BuyTokenDestination, Chain, CoWSwapOnchainOrders::OrderPlacement, ETH_FLOW_PRODUCTION,
    ETH_FLOW_STAGING, GPv2OrderData, OnchainSignature, OrderData, OrderKind, OrderUid,
    SellTokenSource,
};
use nexum::host::{local_store, logging, types};
use shepherd::cow::cow_api;

/// Decoded payload of a `CoWSwapOnchainOrders.OrderPlacement` log.
/// `GPv2OrderData` is ~300 bytes; box it to keep the struct
/// cache-friendly when threaded through the observe path.
#[derive(Debug)]
struct DecodedPlacement {
    /// EthFlow contract that emitted the event. EIP-1271 owner of
    /// the resulting orderbook entry — used as the UID `owner` input.
    contract: Address,
    /// Native-token seller that called `createOrder`. Logged for
    /// operator diagnostics; not the orderbook owner.
    sender: Address,
    order: Box<GPv2OrderData>,
    /// Refund pointer / opaque placer metadata. Recorded by the
    /// orderbook indexer in `ethflowData.userValidTo`; not consumed
    /// by this module.
    #[allow(dead_code)]
    signature: OnchainSignature,
    #[allow(dead_code)]
    data: Bytes,
}

struct EthFlowWatcher;

impl Guest for EthFlowWatcher {
    fn init(_config: Vec<(String, String)>) -> Result<(), HostError> {
        logging::log(logging::Level::Info, "ethflow-watcher init");
        Ok(())
    }

    fn on_event(event: types::Event) -> Result<(), HostError> {
        if let types::Event::Logs(logs) = event {
            for log in &logs {
                if let Some(placement) =
                    decode_order_placement(&log.address, &log.topics, &log.data)
                {
                    observe_placement(log.chain_id, &placement)?;
                }
            }
        }
        // Block / Tick / Message are not used by this module.
        Ok(())
    }
}

// ---- decode (BLEU-832) ----

/// Decode a raw event log against `CoWSwapOnchainOrders.OrderPlacement`.
///
/// Returns `None` when:
/// - the log's contract address is not one of the canonical `ETH_FLOW_*`
///   deployments (defensive — the host's `[[subscription]]` filter
///   already pins the address, but a misconfigured engine could still
///   leak through);
/// - topic0 does not match the event signature; or
/// - the ABI body fails to decode (truncated, wrong layout).
fn decode_order_placement(
    address: &[u8],
    topics: &[Vec<u8>],
    data: &[u8],
) -> Option<DecodedPlacement> {
    if address.len() != 20 {
        return None;
    }
    let contract = Address::from_slice(address);
    if contract != ETH_FLOW_PRODUCTION && contract != ETH_FLOW_STAGING {
        return None;
    }
    let topic0 = topics.first()?;
    if topic0.len() != 32 || B256::from_slice(topic0) != OrderPlacement::SIGNATURE_HASH {
        return None;
    }
    let words: Vec<B256> = topics
        .iter()
        .filter(|t| t.len() == 32)
        .map(|t| B256::from_slice(t))
        .collect();
    let decoded = OrderPlacement::decode_raw_log(words, data).ok()?;
    Some(DecodedPlacement {
        contract,
        sender: decoded.sender,
        order: Box::new(decoded.order),
        signature: decoded.signature,
        data: decoded.data,
    })
}

// ---- observe + verify (BLEU-833 revised — COW-1076) ----

/// Compute the orderbook UID for the placement and confirm the
/// orderbook's native EthFlow indexer picked it up.
///
/// Background (COW-1076): the orderbook backend indexes
/// `OrderPlacement` events server-side and creates the order entry
/// with its dual-validTo bookkeeping (on-chain `validTo = u32::MAX`
/// for chain compatibility, off-chain `ethflowData.userValidTo`
/// derived from the embedded placer payload). The public
/// `POST /api/v1/orders` endpoint applies the generic validity cap
/// and rejects EthFlow shapes with `ExcessiveValidTo`; there is no
/// path through it that produces the same result as the native
/// indexer. This module therefore observes the event and verifies
/// the indexer caught it, instead of submitting.
fn observe_placement(chain_id: u64, placement: &DecodedPlacement) -> Result<(), HostError> {
    let uid_hex = match compute_uid(chain_id, placement) {
        Some(uid) => format!("{uid}"),
        None => {
            logging::log(
                logging::Level::Warn,
                &format!(
                    "ethflow uid build skipped (sender={:#x}): unsupported chain {chain_id} or unknown order marker",
                    placement.sender,
                ),
            );
            return Ok(());
        }
    };

    // Idempotency: once verified, do not re-check on log re-delivery
    // (engine restart, reorg replay, supervisor restart).
    if local_store::get(&format!("observed:{uid_hex}"))?.is_some() {
        return Ok(());
    }

    let path = format!("/api/v1/orders/{uid_hex}");
    match cow_api::request(chain_id, "GET", &path, None) {
        Ok(_) => {
            local_store::set(&format!("observed:{uid_hex}"), &[])?;
            logging::log(
                logging::Level::Info,
                &format!(
                    "ethflow observed {uid_hex} (orderbook indexed, sender={:#x})",
                    placement.sender,
                ),
            );
        }
        Err(err) if err.code == 404 => {
            // Indexer lag is expected immediately after the block lands —
            // shepherd's WebSocket can deliver the log a few hundred
            // milliseconds before the orderbook's own indexer commits.
            // Do not write `observed:` so the next log re-delivery (or a
            // future block-tick poll) can re-check; log at Info to keep
            // the soak dashboard quiet on normal lag.
            logging::log(
                logging::Level::Info,
                &format!(
                    "ethflow not yet indexed {uid_hex} (sender={:#x}); will recheck on re-delivery",
                    placement.sender,
                ),
            );
        }
        Err(err) => {
            logging::log(
                logging::Level::Warn,
                &format!(
                    "ethflow indexer check failed {uid_hex} ({}): {} (sender={:#x})",
                    err.code, err.message, placement.sender,
                ),
            );
        }
    }
    Ok(())
}

fn compute_uid(chain_id: u64, placement: &DecodedPlacement) -> Option<OrderUid> {
    let chain = Chain::try_from(chain_id).ok()?;
    let domain = chain.settlement_domain();
    let order_data = gpv2_to_order_data(&placement.order)?;
    Some(order_data.uid(&domain, placement.contract))
}

/// Lift the on-chain `GPv2OrderData` enum-byte fields into the typed
/// `OrderData` the UID computation needs. Returns `None` if any of
/// the enum bytes carries an unrecognised marker (defensive — every
/// production EthFlow placement uses the canonical markers).
fn gpv2_to_order_data(gpv2: &GPv2OrderData) -> Option<OrderData> {
    Some(OrderData {
        sell_token: gpv2.sellToken,
        buy_token: gpv2.buyToken,
        receiver: (gpv2.receiver != Address::ZERO).then_some(gpv2.receiver),
        sell_amount: gpv2.sellAmount,
        buy_amount: gpv2.buyAmount,
        valid_to: gpv2.validTo,
        app_data: gpv2.appData,
        fee_amount: gpv2.feeAmount,
        kind: OrderKind::from_contract_bytes(gpv2.kind)?,
        partially_fillable: gpv2.partiallyFillable,
        sell_token_balance: SellTokenSource::from_contract_bytes(gpv2.sellTokenBalance)?,
        buy_token_balance: BuyTokenDestination::from_contract_bytes(gpv2.buyTokenBalance)?,
    })
}

export!(EthFlowWatcher);

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{U256, address, hex};
    use alloy_sol_types::SolValue;
    use cowprotocol::OnchainSigningScheme;

    fn sample_order() -> GPv2OrderData {
        GPv2OrderData {
            sellToken: address!("6810e776880C02933D47DB1b9fc05908e5386b96"),
            buyToken: address!("DAE5F1590db13E3B40423B5b5c5fbf175515910b"),
            receiver: address!("DeaDbeefdEAdbeefdEadbEEFdeadbeEFdEaDbeeF"),
            sellAmount: U256::from(1_000_000_u64),
            buyAmount: U256::from(999_u64),
            validTo: 1_700_000_000,
            appData: B256::repeat_byte(0xaa),
            feeAmount: U256::ZERO,
            // Canonical sell-side / ERC20 markers — cowprotocol exposes
            // them as B256 associated constants matching the on-chain
            // `keccak256("sell")` / `keccak256("erc20")` shapes.
            kind: OrderKind::SELL,
            partiallyFillable: false,
            sellTokenBalance: SellTokenSource::ERC20,
            buyTokenBalance: BuyTokenDestination::ERC20,
        }
    }

    fn sample_event() -> (Address, OrderPlacement) {
        let sender = address!("00112233445566778899aabbccddeeff00112233");
        let event = OrderPlacement {
            sender,
            order: sample_order(),
            signature: OnchainSignature {
                scheme: OnchainSigningScheme::Eip1271,
                data: hex!("c0ffeec0ffeec0ffee").to_vec().into(),
            },
            data: hex!("deadbeef").to_vec().into(),
        };
        (sender, event)
    }

    /// Build `(topics, data)` the way the EVM would emit them. The
    /// indexed `sender` becomes topic1 (left-padded address); the three
    /// non-indexed fields become the abi-encoded body.
    fn encode_log(event: &OrderPlacement) -> (Vec<Vec<u8>>, Vec<u8>) {
        let mut sender_topic = vec![0u8; 12];
        sender_topic.extend_from_slice(event.sender.as_slice());
        let topics = vec![OrderPlacement::SIGNATURE_HASH.to_vec(), sender_topic];
        let data = (
            event.order.clone(),
            event.signature.clone(),
            event.data.clone(),
        )
            .abi_encode_params();
        (topics, data)
    }

    // ---- decode tests (BLEU-832) ----

    #[test]
    fn decodes_well_formed_placement() {
        let (sender, event) = sample_event();
        let (topics, data) = encode_log(&event);
        let address = ETH_FLOW_PRODUCTION.as_slice();

        let decoded = decode_order_placement(address, &topics, &data).expect("decode succeeds");
        assert_eq!(decoded.contract, ETH_FLOW_PRODUCTION);
        assert_eq!(decoded.sender, sender);
        assert_eq!(decoded.order.sellToken, event.order.sellToken);
        assert_eq!(decoded.order.buyAmount, event.order.buyAmount);
        assert_eq!(decoded.signature.scheme, OnchainSigningScheme::Eip1271);
        assert_eq!(
            decoded.signature.data.as_ref(),
            event.signature.data.as_ref()
        );
        assert_eq!(decoded.data.as_ref(), event.data.as_ref());
    }

    #[test]
    fn accepts_staging_address() {
        let (_, event) = sample_event();
        let (topics, data) = encode_log(&event);
        assert!(decode_order_placement(ETH_FLOW_STAGING.as_slice(), &topics, &data).is_some());
    }

    #[test]
    fn rejects_unrelated_contract_address() {
        let (_, event) = sample_event();
        let (topics, data) = encode_log(&event);
        let stranger = address!("dead00000000000000000000000000000000dead");
        assert!(decode_order_placement(stranger.as_slice(), &topics, &data).is_none());
    }

    #[test]
    fn rejects_wrong_topic_signature() {
        let (_, event) = sample_event();
        let (_, data) = encode_log(&event);
        let bad_topic = vec![0xaa_u8; 32];
        let sender_topic = vec![0u8; 32];
        assert!(
            decode_order_placement(
                ETH_FLOW_PRODUCTION.as_slice(),
                &[bad_topic, sender_topic],
                &data,
            )
            .is_none()
        );
    }

    #[test]
    fn rejects_truncated_address() {
        let (_, event) = sample_event();
        let (topics, data) = encode_log(&event);
        assert!(decode_order_placement(&[0u8; 19], &topics, &data).is_none());
    }

    #[test]
    fn rejects_truncated_data() {
        let (topics, _) = encode_log(&sample_event().1);
        assert!(decode_order_placement(ETH_FLOW_PRODUCTION.as_slice(), &topics, &[]).is_none());
    }

    #[test]
    fn rejects_empty_topics() {
        let (_, data) = encode_log(&sample_event().1);
        assert!(decode_order_placement(ETH_FLOW_PRODUCTION.as_slice(), &[], &data).is_none());
    }

    // ---- UID computation (observe path) ----

    /// Sanity check that `compute_uid` produces the canonical 56-byte
    /// shape `digest || owner || valid_to`. We do not pin the exact UID
    /// (it depends on `Chain::SEPOLIA::settlement_domain()` and would
    /// drift if cowprotocol's domain separator changes); instead we
    /// assert structural invariants the orderbook joins on.
    #[test]
    fn compute_uid_pins_owner_to_ethflow_contract_and_validto() {
        let (_, event) = sample_event();
        let (topics, data) = encode_log(&event);
        let decoded =
            decode_order_placement(ETH_FLOW_PRODUCTION.as_slice(), &topics, &data).unwrap();

        // 11155111 = Sepolia chain ID; supported by cowprotocol::Chain.
        let uid = compute_uid(11_155_111, &decoded).expect("known chain + markers");
        let bytes: [u8; 56] = uid.into();

        // owner suffix (bytes 32..52) = EthFlow contract address.
        assert_eq!(&bytes[32..52], ETH_FLOW_PRODUCTION.as_slice());
        // valid_to suffix (bytes 52..56) = u32 big-endian of the order's
        // on-chain validTo. EthFlow uses u32::MAX in production; this
        // sample uses 1_700_000_000.
        assert_eq!(
            u32::from_be_bytes(bytes[52..56].try_into().unwrap()),
            event.order.validTo,
        );
    }

    #[test]
    fn compute_uid_returns_none_on_unsupported_chain() {
        let (_, event) = sample_event();
        let (topics, data) = encode_log(&event);
        let decoded =
            decode_order_placement(ETH_FLOW_PRODUCTION.as_slice(), &topics, &data).unwrap();
        // Chain id 9999 is not in `cowprotocol::Chain`.
        assert!(compute_uid(9999, &decoded).is_none());
    }

    #[test]
    fn gpv2_to_order_data_rejects_unknown_kind_marker() {
        let mut order = sample_order();
        order.kind = B256::repeat_byte(0xff);
        assert!(gpv2_to_order_data(&order).is_none());
    }
}

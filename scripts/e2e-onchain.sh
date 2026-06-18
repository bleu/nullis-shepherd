#!/usr/bin/env bash
# scripts/e2e-onchain.sh — execute the on-chain side of the COW-1064
# E2E run.
#
# Pre-flight:
#   - derive the EOA address from $OPERATOR_PRIVATE_KEY
#     and assert it matches the pinned $TEST_EOA;
#   - assert balance ≥ 0.02 ETH (covers 2 tx + slippage).
#
# Required actions (cover twap-monitor + ethflow-watcher markers):
#   1. ComposableCoW.create(...) — fires ConditionalOrderCreated;
#      uses the 516-byte calldata pinned in lib.sh /
#      e2e-cow-1064-prep.md so the TWAP order shape is reproducible.
#   2. EthFlow.createOrder(EthFlowOrder.Data) — fires OrderPlacement;
#      tuple built dynamically from the cow.fi /quote response (the
#      `quoteId` + `feeAmount` only exist after a quote, so this part
#      is not pinned to a constant).
#
# Optional path (only if $RUN_OPTIONAL_PRESIGN=1 in scripts/.env):
#   3. WETH9.deposit() — wrap 0.01 ETH so stop-loss has a sell-side
#      balance.
#   4. setPreSignature($EXPECTED_ORDER_UID, true) — enables the
#      already-submitted stop-loss order for settlement.
#   5. WETH9.approve(GPv2VaultRelayer, 0.005 ETH) — sell-side
#      allowance.
#
# Output: each tx hash is appended to scripts/.state under
# TX_<KIND>=0x<hash> so the report generator can link them.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "$SCRIPT_DIR/lib.sh"

require_cmd cast
require_cmd curl
require_cmd python3

load_env
[[ -n "${OPERATOR_PRIVATE_KEY:-}" ]] || die "OPERATOR_PRIVATE_KEY unset in scripts/.env"

derived="$(cast wallet address --private-key "$OPERATOR_PRIVATE_KEY" 2>/dev/null)" \
    || die "cast wallet address failed — is OPERATOR_PRIVATE_KEY a valid 0x-prefixed 32-byte hex?"
if [[ "${derived,,}" != "${TEST_EOA,,}" ]]; then
    die "private key derives to $derived, expected $TEST_EOA — wrong EOA loaded"
fi
log "EOA: $derived"

balance="$(cast balance "$TEST_EOA" --rpc-url "$RPC_URL_SEPOLIA_HTTP")"
log "EOA balance: $(python3 -c "print(f'{int(\"$balance\")/1e18:.6f} ETH')") ($balance wei)"
if (( balance < 20000000000000000 )); then  # 0.02 ETH
    die "EOA balance < 0.02 ETH — top up from a Sepolia faucet first"
fi

# ── Action 1: ComposableCoW.create() ─────────────────────────────────

twap_calldata="0x6bfae1ca000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000010000000000000000000000006cf1e9ca41f7611def408122793c358a3d11e5a5000000000000000000000000000000000000000000000000000000006670f00000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000140000000000000000000000000fff9976782d46cc05630d1f6ebab18b2324d6b140000000000000000000000000625afb445c3b6b7b929342a04a22599fd5dbb5900000000000000000000000014995a1118caf95833e923faf8dd155721cd53c200000000000000000000000000000000000000000000000000038d7ea4c6800000000000000000000000000000000000000000000000000006f05b59d3b2000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000025800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"

log "submitting TWAP ComposableCoW.create() → $COMPOSABLE_COW"
tx_twap="$(cast send \
    --rpc-url    "$RPC_URL_SEPOLIA_HTTP" \
    --private-key "$OPERATOR_PRIVATE_KEY" \
    --json \
    "$COMPOSABLE_COW" \
    "$twap_calldata" \
    | jq -r '.transactionHash')"
[[ "$tx_twap" =~ ^0x[a-fA-F0-9]{64}$ ]] || die "TWAP tx hash malformed: $tx_twap"
log "  TWAP tx: $tx_twap"
log "  Etherscan: https://sepolia.etherscan.io/tx/$tx_twap"
write_state "TX_TWAP=$tx_twap"

# ── Action 2: EthFlow.createOrder() ──────────────────────────────────

log "fetching cow.fi /quote for EthFlow swap (0.005 ETH → COW)"
quote_out="$(python3 "$SCRIPT_DIR/_ethflow_quote.py" "$TEST_EOA" 5000000000000000)" \
    || die "EthFlow quote helper failed"
ethflow_calldata="$(echo "$quote_out" | grep '^CALLDATA=' | cut -d= -f2-)"
ethflow_value="$(echo "$quote_out" | grep '^VALUE_WEI=' | cut -d= -f2)"
[[ "$ethflow_calldata" =~ ^0x[a-fA-F0-9]+$ ]] || die "EthFlow calldata malformed"
[[ "$ethflow_value" =~ ^[0-9]+$ ]] || die "EthFlow value malformed: $ethflow_value"
log "  msg.value = $ethflow_value wei ($(python3 -c "print(f'{$ethflow_value/1e18:.6f} ETH')"))"

log "submitting EthFlow.createOrder() → $ETHFLOW"
tx_ethflow="$(cast send \
    --rpc-url    "$RPC_URL_SEPOLIA_HTTP" \
    --private-key "$OPERATOR_PRIVATE_KEY" \
    --value      "$ethflow_value" \
    --json \
    "$ETHFLOW" \
    "$ethflow_calldata" \
    | jq -r '.transactionHash')"
[[ "$tx_ethflow" =~ ^0x[a-fA-F0-9]{64}$ ]] || die "EthFlow tx hash malformed: $tx_ethflow"
log "  EthFlow tx: $tx_ethflow"
log "  Etherscan: https://sepolia.etherscan.io/tx/$tx_ethflow"
write_state "TX_ETHFLOW=$tx_ethflow"

# ── Optional actions ─────────────────────────────────────────────────

if [[ "${RUN_OPTIONAL_PRESIGN:-0}" -eq 1 ]]; then
    log "RUN_OPTIONAL_PRESIGN=1 → wrap WETH + setPreSignature + approve"

    log "  WETH9.deposit() — wrapping 0.01 ETH"
    tx_wrap="$(cast send --rpc-url "$RPC_URL_SEPOLIA_HTTP" --private-key "$OPERATOR_PRIVATE_KEY" --value 10000000000000000 --json "$WETH_SEPOLIA" "deposit()" | jq -r '.transactionHash')"
    log "    tx: $tx_wrap"
    write_state "TX_WRAP=$tx_wrap"

    log "  GPv2Settlement.setPreSignature($EXPECTED_ORDER_UID, true)"
    tx_presign="$(cast send --rpc-url "$RPC_URL_SEPOLIA_HTTP" --private-key "$OPERATOR_PRIVATE_KEY" --json "$GPV2_SETTLEMENT" "setPreSignature(bytes,bool)" "$EXPECTED_ORDER_UID" true | jq -r '.transactionHash')"
    log "    tx: $tx_presign"
    write_state "TX_PRESIGN=$tx_presign"

    log "  WETH9.approve(GPv2VaultRelayer, 0.005 ETH)"
    tx_approve="$(cast send --rpc-url "$RPC_URL_SEPOLIA_HTTP" --private-key "$OPERATOR_PRIVATE_KEY" --json "$WETH_SEPOLIA" "approve(address,uint256)" "$GPV2_VAULT_RELAYER" 5000000000000000 | jq -r '.transactionHash')"
    log "    tx: $tx_approve"
    write_state "TX_APPROVE=$tx_approve"
else
    log "RUN_OPTIONAL_PRESIGN=0 → skipping wrap/setPreSignature/approve"
    log "  (stop-loss still produces submitted:{uid} via the CoW orderbook"
    log "   pre-sign acceptance path; flip to 1 in .env to also enable on-chain settlement.)"
fi

log "done. tail the engine log to watch markers land:"
log "  tail -F $(state_value LOG_FILE)"

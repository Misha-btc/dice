#!/usr/bin/env bash
# Cheeky Bricky — Regtest Deployment
#
# Deploys cheeky-bricky-game as a single immutable direct contract at 4:205 (0xCD).
# Init (opcode 0) runs at deploy time via the cellpack — the
# standard parameterized-init pattern for immutable alkanes.
#
# Usage:
#   ./scripts/deploy-regtest.sh               # Deploy on regtest
#   ./scripts/deploy-regtest.sh --build       # Build WASM first

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR/.."
WASM_DIR="$PROJECT_DIR/target/wasm32-unknown-unknown/release"
source "$SCRIPT_DIR/_env.sh"

# Bet/bank token comes from _env.sh (regtest default: DIESEL 2:0; mainnet: 2:490
# via BET_TOKEN_BLOCK=2 BET_TOKEN_TX=490). min_bet is a compile-time 1000.

if [ "${1:-}" = "--build" ]; then
    bash "$SCRIPT_DIR/build-wasms.sh"
    echo ""
fi

height() {
    python3 -c "import json,urllib.request;r=json.loads(urllib.request.urlopen(urllib.request.Request('$RPC_URL',data=json.dumps({'jsonrpc':'2.0','id':1,'method':'metashrew_height','params':[]}).encode(),headers={'Content-Type':'application/json'})).read());print(int(r['result']))"
}

wait_for_height() {
    local target=$1
    python3 -c "
import json,urllib.request,time
while True:
    try:
        r=json.loads(urllib.request.urlopen(urllib.request.Request('$RPC_URL',data=json.dumps({'jsonrpc':'2.0','id':1,'method':'metashrew_height','params':[]}).encode(),headers={'Content-Type':'application/json'})).read())
        if int(r['result'])>=$target:break
    except:pass
    time.sleep(1)
"
}

check_success() {
    local name="$1" result="$2"
    if echo "$result" | grep -q "completed successfully"; then
        echo "  ✓ $name"
    else
        echo "  ✗ $name FAILED"
        echo "$result" | grep -E "Error|error" | head -3
        exit 1
    fi
}

echo "═══════════════════════════════════════"
echo "  Cheeky Bricky — Regtest Deployment"
echo "  Network: $NETWORK / RPC: $RPC_URL"
echo "═══════════════════════════════════════"
echo ""

if [ "$NETWORK" = "regtest" ]; then
    echo "Setting up wallet..."
    if [ ! -f "$WALLET_FILE" ]; then
        $CLI wallet create > /dev/null 2>&1
    fi
    $CLI bitcoind generatetoaddress 301 "p2tr:0" > /dev/null 2>&1
    wait_for_height 300
    echo "  ✓ Wallet funded"
    echo ""
fi

echo "Deploying cheeky-bricky-operator (fee-withdrawal auth key, 4:$OPERATOR_TX)..."
# Init (opcode 0) mints ONE auth-key unit to your deploy address — guard it,
# it's the only key that can withdraw a table's fees. Deploy BEFORE the game so
# the game's hardcoded OPERATOR_AUTH_ID resolves.
check_success "cheeky-bricky-operator (4:$OPERATOR_TX) + init" \
    "$(run "[3,$OPERATOR_TX,0]:v0:v0" "" "$WASM_DIR/cheeky_bricky_operator.wasm")"
echo ""

echo "Deploying cheeky-bricky-game (with init, bet token $BET_TOKEN_BLOCK:$BET_TOKEN_TX)..."
# Last two args: share-token name/symbol (u128-packed; 0,0 = defaults).
check_success "cheeky-bricky-game (4:$GAME_TX) + init" \
    "$(run "[3,$GAME_TX,0,$BET_TOKEN_BLOCK,$BET_TOKEN_TX,0,0]:v0:v0" "" "$WASM_DIR/cheeky_bricky_game.wasm")"
echo ""

if [ "$NETWORK" = "regtest" ]; then
    echo "Mining DIESEL..."
    run "[2,0,77]:v0:v0" "" "" > /dev/null 2>&1
    echo "  ✓ DIESEL mined (free mint, ~312.5M per block)"
fi

echo ""
echo "═══════════════════════════════════════"
echo "  Cheeky Bricky Deployment Complete!"
echo ""
echo "  cheeky-bricky-game (singleton + share token): 4:$GAME_TX"
echo "  bet/bank token (immutable):             $BET_TOKEN_BLOCK:$BET_TOKEN_TX"
echo ""
echo "  Game constants (from cheeky-bricky-math):"
echo "    EDGE:   1% — fixed forever, ALL of it to LPs (no protocol fee, no admin)"
echo "    KELLY:  0.5% of bank max net win per bet"
echo "    BLOCK:  20% of bank max net payouts per block (miner defense)"
echo "    TARGET: 656..64238  (1%..98% win chance)"
echo ""
echo "  operator auth key (fee withdrawal):     4:$OPERATOR_TX (minted to you)"
echo "    LP exit fee: 2% of each withdrawal → the table's fee_accum,"
echo "    collected by the operator via WithdrawFees (op 6, auth-gated)"
echo ""
echo "  Next steps:"
echo "    ./scripts/deposit.sh 100000000        # seed the bank (mint CBRK shares)"
echo "    ./scripts/roll.sh 32768 1000000       # bet 0.01 DIESEL on a coin flip"
echo "    ./scripts/view.sh bank                # bank / shares / fee_accum"
echo "    ./scripts/withdraw-fees.sh            # operator collects the 2% exit fees"
echo "═══════════════════════════════════════"

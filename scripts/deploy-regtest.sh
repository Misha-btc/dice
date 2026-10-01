#!/usr/bin/env bash
# Dice — Regtest Deployment
#
# Deploys dice-game as a single immutable direct contract at 4:64206 (0xFACE).
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
echo "  Dice — Regtest Deployment"
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

echo "Deploying dice-operator (fee-withdrawal auth key, 4:$OPERATOR_TX)..."
# Init (opcode 0) mints THREE interchangeable auth-key units to your deploy
# address — any one can withdraw fees / set the exit fee on every table, so keep
# the spares apart and guard all three. Deploy BEFORE the game so
# the game's hardcoded OPERATOR_AUTH_ID resolves.
check_success "dice-operator (4:$OPERATOR_TX) + init" \
    "$(run "[3,$OPERATOR_TX,0]:v0:v0" "" "$WASM_DIR/dice_operator.wasm")"
echo ""

echo "Deploying dice-game (with init, bet token $BET_TOKEN_BLOCK:$BET_TOKEN_TX)..."
# Last two args: share-token name/symbol (u128-packed; 0,0 = defaults).
check_success "dice-game (4:$GAME_TX) + init" \
    "$(run "[3,$GAME_TX,0,$BET_TOKEN_BLOCK,$BET_TOKEN_TX,0,0]:v0:v0" "" "$WASM_DIR/dice_game.wasm")"
echo ""

if [ "$NETWORK" = "regtest" ]; then
    echo "Mining DIESEL..."
    run "[2,0,77]:v0:v0" "" "" > /dev/null 2>&1
    echo "  ✓ DIESEL mined (free mint, ~312.5M per block)"
fi

echo ""
echo "═══════════════════════════════════════"
echo "  Dice Deployment Complete!"
echo ""
echo "  dice-game (singleton + share token): 4:$GAME_TX"
echo "  bet/bank token (immutable):             $BET_TOKEN_BLOCK:$BET_TOKEN_TX"
echo ""
echo "  Game constants (from dice-math):"
echo "    EDGE:   1% — fixed forever, ALL of it to LPs (players pay no house rake)"
echo "    KELLY:  1% of bank max net win per bet (full Kelly, growth-optimal)"
echo "    BLOCK:  20% of bank max net payouts per block (miner defense)"
echo "    TARGET: 1..64238  (0.0015%..98% win chance; max multiplier ~x64880)"
echo "    EXIT:   2% LP-exit fee, operator-adjustable [0..2%] via SetExitFee"
echo ""
echo "  operator auth keys (fee withdrawal):    3 × 4:$OPERATOR_TX (minted to you)"
echo "    LP exit fee: 2% of each withdrawal → the table's fee_accum,"
echo "    collected by the operator via WithdrawFees (op 6, auth-gated)"
echo ""
echo "  Next steps:"
echo "    ./scripts/deposit.sh 100000000        # seed the bank (mint DICE shares)"
echo "    ./scripts/roll.sh 32768 1000000       # bet 0.01 DIESEL on a coin flip"
echo "    ./scripts/view.sh bank                # bank / shares / fee_accum / exit_bps"
echo "    ./scripts/withdraw-fees.sh            # operator collects the exit fees"
echo "    ./scripts/set-exit-fee.sh 100         # operator lowers the fee to 1% (max 2%)"
echo "═══════════════════════════════════════"

#!/usr/bin/env bash
# Shared environment for Dice regtest scripts. Source, don't execute.
# Mirrors arbuzino's per-script boilerplate (alkanes-cli resolution etc.).

GAME_BLOCK=4
GAME_TX=64206              # 0xFACE — the same slot as mainnet
# Operator auth key — the game's hardcoded WithdrawFees gate (OPERATOR_AUTH_ID
# = 4:0xDEAD). Must match the runtime constant; deploy it BEFORE the game. The
# deployer receives three interchangeable key units (any one authorizes).
OPERATOR_BLOCK=4
OPERATOR_TX=57005          # 0xDEAD
# Bet/bank currency (immutable init param of the game). Regtest defaults to
# DIESEL 2:0 (the only free-mintable token there); MAINNET uses 2:69 (H2S) —
# override with BET_TOKEN_BLOCK/BET_TOKEN_TX.
BET_TOKEN_BLOCK="${BET_TOKEN_BLOCK:-2}"
BET_TOKEN_TX="${BET_TOKEN_TX:-0}"

ALKANES_RS="${ALKANES_RS:-$HOME/subfrost/alkanes-rs}"
ALKANES_CLI="${ALKANES_CLI:-}"
if [ -z "$ALKANES_CLI" ]; then
    for candidate in \
        "$ALKANES_RS/target/release/alkanes-cli" \
        "$(which alkanes-cli 2>/dev/null)"; do
        [ -x "$candidate" ] && ALKANES_CLI="$candidate" && break
    done
fi
[ -z "$ALKANES_CLI" ] && { echo "ERROR: alkanes-cli not found (set ALKANES_CLI= or ALKANES_RS=)"; exit 1; }

NETWORK="${NETWORK:-regtest}"
RPC_URL="${RPC_URL:-http://127.0.0.1:18888}"
WALLET_FILE="${WALLET_FILE:-$HOME/.alkanes/wallet-dice.json}"
DEPLOY_PASSWORD="${DEPLOY_PASSWORD:-testtest}"
# bitcoind itself (docker regtest creds): bulk mining goes here directly —
# alkanes-jsonrpc caps generatetoaddress at 1 block per call.
BITCOIN_RPC_URL="${BITCOIN_RPC_URL:-http://bitcoinrpc:bitcoinrpc@127.0.0.1:18443}"

CLI="$ALKANES_CLI -p $NETWORK --jsonrpc-url $RPC_URL --wallet-file $WALLET_FILE --passphrase $DEPLOY_PASSWORD"

# Execute a cellpack; args: proto [inputs] [envelope]
run() {
    local proto="$1" inputs="${2:-}" envelope="${3:-}"
    local cmd="echo y | $CLI alkanes execute \"$proto\""
    [ -n "$inputs" ] && cmd="$cmd --inputs \"$inputs\""
    [ -n "$envelope" ] && cmd="$cmd --envelope \"$envelope\""
    cmd="$cmd --from p2tr:0 --change p2tr:0 --alkanes-change p2tr:0 --mine --fee-rate 1"
    eval "$cmd" 2>&1
}

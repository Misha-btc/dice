#!/usr/bin/env bash
# Owner-only: withdraw AMOUNT of a token from the operator treasury.
# Requires the treasury owner key (1 unit of 4:TREASURY_TX, minted to you at
# deploy) — it's spent into the call and returned alongside the payout.
#
# Usage: ./scripts/treasury-withdraw.sh AMOUNT [TOKEN_BLOCK TOKEN_TX]
#   TOKEN defaults to the bet/bank token (the swept exit fees).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

if [ $# -lt 1 ]; then
    echo "Usage: $0 AMOUNT [TOKEN_BLOCK TOKEN_TX]"
    exit 1
fi
AMOUNT=$1
TOKEN_BLOCK="${2:-$BET_TOKEN_BLOCK}"
TOKEN_TX="${3:-$BET_TOKEN_TX}"

echo "Withdrawing $AMOUNT of $TOKEN_BLOCK:$TOKEN_TX from treasury (owner key required)..."
run "[$TREASURY_BLOCK,$TREASURY_TX,2,$TOKEN_BLOCK,$TOKEN_TX,$AMOUNT]:v0:v0" \
    "$TREASURY_BLOCK:$TREASURY_TX:1"

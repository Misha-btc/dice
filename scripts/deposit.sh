#!/usr/bin/env bash
# LP in: deposit AMOUNT of the bet token into the bank, minting CBRK shares at the
# current share price.
#
# Usage: ./scripts/deposit.sh AMOUNT

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

if [ $# -lt 1 ]; then
    echo "Usage: $0 AMOUNT"
    exit 1
fi
AMOUNT=$1

echo "Depositing $AMOUNT of the bet token into the bank..."
run "[$GAME_BLOCK,$GAME_TX,2,$AMOUNT]:v0:v0" "$BET_TOKEN_BLOCK:$BET_TOKEN_TX:$AMOUNT"

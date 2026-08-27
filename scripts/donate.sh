#!/usr/bin/env bash
# Gift AMOUNT of the bet token to the bank without minting shares (seeding/marketing —
# every outstanding CHKD share appreciates).
#
# Usage: ./scripts/donate.sh AMOUNT

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

if [ $# -lt 1 ]; then
    echo "Usage: $0 AMOUNT"
    exit 1
fi
AMOUNT=$1

echo "Donating $AMOUNT of the bet token to the bank..."
run "[$GAME_BLOCK,$GAME_TX,5,$AMOUNT]:v0:v0" "$BET_TOKEN_BLOCK:$BET_TOKEN_TX:$AMOUNT"

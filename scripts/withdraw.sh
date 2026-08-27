#!/usr/bin/env bash
# LP out: burn SHARES (CHKD, units of 4:205) for their pro-rata slice of the
# bank. MIN_PAYOUT is a slippage floor (0 = off).
#
# Usage: ./scripts/withdraw.sh SHARES [MIN_PAYOUT]

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

if [ $# -lt 1 ]; then
    echo "Usage: $0 SHARES [MIN_PAYOUT]"
    exit 1
fi
SHARES=$1
MIN_PAYOUT="${2:-0}"

echo "Withdrawing: burning $SHARES shares (min_payout=$MIN_PAYOUT)..."
run "[$GAME_BLOCK,$GAME_TX,3,$SHARES,$MIN_PAYOUT]:v0:v0" "$GAME_BLOCK:$GAME_TX:$SHARES"

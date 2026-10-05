#!/usr/bin/env bash
# LP out: burn SHARES (DICE, units of 4:64206) for their pro-rata slice of the
# bank, MINUS the LP-exit fee in force (operator-set within 0..2%, currently 2%;
# the fee accrues in the table's fee_accum and is later collected by the
# operator). MIN_PAYOUT is a slippage
# floor on the NET amount you receive (0 = off).
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

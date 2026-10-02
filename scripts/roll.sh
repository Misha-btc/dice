#!/usr/bin/env bash
# Place a bet: roll under TARGET for AMOUNT of the bet token. Settles in this
# same tx — check the trace/balance after mining to see the outcome.
# An oversized bet is CLAMPED down to what the caps allow (excess returned);
# pass MIN_AMOUNT to revert instead if the clamp falls below it (0 = accept).
#
# Usage: ./scripts/roll.sh TARGET AMOUNT [MIN_AMOUNT]
#   TARGET ∈ [1, t_max] (see ./scripts/view.sh config) — win chance TARGET/65536
#   AMOUNT — bet-token base units (≥ min_bet; cf. view.sh max-bet TARGET)

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

if [ $# -lt 2 ]; then
    echo "Usage: $0 TARGET AMOUNT [MIN_AMOUNT]"
    exit 1
fi
TARGET=$1
AMOUNT=$2
MIN_AMOUNT="${3:-0}"

echo "Rolling under $TARGET for $AMOUNT of the bet token (min_amount=$MIN_AMOUNT)..."
run "[$GAME_BLOCK,$GAME_TX,1,$TARGET,$AMOUNT,$MIN_AMOUNT]:v0:v0" "$BET_TOKEN_BLOCK:$BET_TOKEN_TX:$AMOUNT"

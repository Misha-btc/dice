#!/usr/bin/env bash
# Operator: withdraw a table's accrued 2% LP-exit fees (in the bet token).
# Requires the operator auth key (1 unit of 4:OPERATOR_TX, minted to you at
# deploy) — it's routed into the call and returned alongside the fee payout.
# One key drains every table you run; pass a different TABLE for a clone.
#
# Usage: ./scripts/withdraw-fees.sh [TABLE_BLOCK TABLE_TX]
#   TABLE defaults to the canonical game (4:GAME_TX).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

TABLE_BLOCK="${1:-$GAME_BLOCK}"
TABLE_TX="${2:-$GAME_TX}"

echo "Withdrawing accrued exit fees from table $TABLE_BLOCK:$TABLE_TX (operator key required)..."
run "[$TABLE_BLOCK,$TABLE_TX,6]:v0:v0" "$OPERATOR_BLOCK:$OPERATOR_TX:1"

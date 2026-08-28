#!/usr/bin/env bash
# Operator: set a table's LP-exit fee to BPS (0..=200; 200 = 2% = the immutable
# max, 0 = free exits). Requires the operator auth key (1 unit of
# 4:OPERATOR_TX) — routed in and returned. One key works on every table.
#
# Usage: ./scripts/set-exit-fee.sh BPS [TABLE_BLOCK TABLE_TX]
#   TABLE defaults to the canonical game (4:GAME_TX).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

if [ $# -lt 1 ]; then
    echo "Usage: $0 BPS [TABLE_BLOCK TABLE_TX]   (BPS 0..=200)"
    exit 1
fi
BPS=$1
TABLE_BLOCK="${2:-$GAME_BLOCK}"
TABLE_TX="${3:-$GAME_TX}"

echo "Setting exit fee of table $TABLE_BLOCK:$TABLE_TX to $BPS bps (operator key required)..."
run "[$TABLE_BLOCK,$TABLE_TX,7,$BPS]:v0:v0" "$OPERATOR_BLOCK:$OPERATOR_TX:1"

#!/usr/bin/env bash
# Permissionless: create the CANONICAL table for a token via the registry
# (opcode 4 on the template). One table per token, forever. Payable in the
# token: SEED units (>= 1000) ride along and become YOUR first LP deposit in
# the newborn table (you get its share units back). The table's name/symbol
# are read from the token itself (99/100) — nothing to pass.
#
# Usage: ./scripts/create-table.sh TOKEN_BLOCK TOKEN_TX SEED

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

if [ $# -lt 3 ]; then
    echo "Usage: $0 TOKEN_BLOCK TOKEN_TX SEED   # SEED >= 1000 units of the token"
    exit 1
fi
TB=$1
TT=$2
SEED=$3

echo "Creating the canonical table for $TB:$TT (seeding $SEED units as your LP)..."
run "[$GAME_BLOCK,$GAME_TX,4,$TB,$TT]:v0:v0" "$TB:$TT:$SEED"
echo "Resolve it with: ./scripts/view.sh table-of $TB $TT"

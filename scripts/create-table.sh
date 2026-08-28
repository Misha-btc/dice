#!/usr/bin/env bash
# Permissionless: create the CANONICAL table for a token via the registry
# (opcode 4 on the template). One table per token, forever. NOT payable and
# NOT seeded — the table is born empty (dormant until someone deposits), like
# the template's own table. The name/symbol are read from the token itself
# (99/100); nothing to pass but the token id.
#
# Usage: ./scripts/create-table.sh TOKEN_BLOCK TOKEN_TX

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

if [ $# -lt 2 ]; then
    echo "Usage: $0 TOKEN_BLOCK TOKEN_TX"
    exit 1
fi
TB=$1
TT=$2

echo "Creating the canonical table for $TB:$TT (empty; seed it later with deposit.sh)..."
run "[$GAME_BLOCK,$GAME_TX,4,$TB,$TT]:v0:v0"
echo "Resolve it with: ./scripts/view.sh table-of $TB $TT"

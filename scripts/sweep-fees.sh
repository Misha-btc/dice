#!/usr/bin/env bash
# Sweep the game's accrued 2% LP-exit fees into the operator treasury.
# Permissionless (anyone can trigger it) — the destination is the game's
# hardcoded TREASURY_ID, so it can only ever land in the operator's collector.
# Reverts if there is nothing to sweep.
#
# Usage: ./scripts/sweep-fees.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

echo "Sweeping accrued exit fees → treasury 4:$TREASURY_TX ..."
run "[$GAME_BLOCK,$GAME_TX,6]:v0:v0"

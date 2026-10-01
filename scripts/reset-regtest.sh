#!/usr/bin/env bash
# Полный reset Docker-regtest окружения для чистого деплоя.
#
# `alkanes-rs` поднимает bitcoind + metashrew через docker-compose.
# Здесь мы:
#   1. docker compose down -v   (полный wipe state)
#   2. docker compose up -d     (свежий старт)
#   3. ждём пока metashrew_height станет ≤ 1 (значит DB чистая)
#
# Используется ПЕРЕД ./scripts/deploy-regtest.sh — особенно после
# изменения immutable констант (POOL_*_BPS, multipliers и т.п.).
#
# Env:
#   ALKANES_RS    — путь к каталогу alkanes-rs (где лежит docker-compose.yml)
#   RPC_URL       — default http://127.0.0.1:18888

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR/.."
ALKANES_RS="${ALKANES_RS:-$(cd "$PROJECT_DIR/.." && pwd)/alkanes-rs}"
RPC_URL="${RPC_URL:-http://127.0.0.1:18888}"

RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'

# ── Sanity checks ─────────────────────────────────────
if [ ! -f "$ALKANES_RS/docker-compose.yml" ] \
   && [ ! -f "$ALKANES_RS/docker-compose.yaml" ] \
   && [ ! -f "$ALKANES_RS/compose.yml" ] \
   && [ ! -f "$ALKANES_RS/compose.yaml" ]; then
    echo -e "${RED}ERROR: docker-compose file not found at $ALKANES_RS${NC}"
    echo "       Set ALKANES_RS=/path/to/alkanes-rs"
    exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
    echo -e "${RED}ERROR: docker not found in PATH${NC}"
    exit 1
fi

echo -e "${CYAN}═══ Restarting regtest (clean) ═══${NC}"
echo "  alkanes-rs: $ALKANES_RS"
echo "  RPC:        $RPC_URL"
echo ""

# ── Tear down + bring up ──────────────────────────────
echo "  docker compose down -v..."
(cd "$ALKANES_RS" && docker compose down -v > /dev/null 2>&1) || true
echo "  docker compose up -d..."
(cd "$ALKANES_RS" && docker compose up -d > /dev/null 2>&1)

# ── Wait for metashrew_height ≤ 1 (signals clean DB) ──
echo "  Waiting for metashrew to come up clean..."
sleep 3
python3 - <<PYEOF
import json, urllib.request, time, sys
ok = False
for i in range(60):
    try:
        req = urllib.request.Request(
            "$RPC_URL",
            data=json.dumps({"jsonrpc": "2.0", "id": 1,
                             "method": "metashrew_height", "params": []}).encode(),
            headers={"Content-Type": "application/json"},
        )
        r = json.loads(urllib.request.urlopen(req, timeout=3).read())
        h = int(r["result"])
        if h <= 1:
            ok = True
            print(f"    height={h} — clean")
            break
    except Exception:
        pass
    time.sleep(2)
if not ok:
    print("WARNING: regtest may not be clean (height > 1 or RPC unreachable)")
    sys.exit(1)
PYEOF

# Also wipe our wallet so deploy creates a fresh one
WALLET_FILE="${WALLET_FILE:-$HOME/.alkanes/wallet-dice.json}"
if [ -f "$WALLET_FILE" ]; then
    rm -f "$WALLET_FILE"
    echo "  wallet wiped: $WALLET_FILE"
fi
# ...and deploy-mainnet.sh's regtest rehearsal record (its txids died with the chain)
rm -f "$PROJECT_DIR/target/deploy-regtest.env"

echo ""
echo -e "${CYAN}═══ Reset Complete ═══${NC}"
echo ""
echo "  Next:"
echo "    ./scripts/deploy-regtest.sh --build"

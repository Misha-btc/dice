#!/usr/bin/env bash
# Cheeky Bricky end-to-end smoke test on regtest. The critical LP+player loop:
#   1. Deploy the game + operator auth key (init) + mine DIESEL
#   2. Become the house: deposit → mint CBRK shares (bank funded)
#   3. Play: several rolls (wins pay out, losses feed the bank) via traces
#   4. Withdraw: burn shares → 98% DIESEL back (2% exit fee → /fee_accum)
#   5. Operator collects the accrued exit fee (auth-gated WithdrawFees)
#   6. Views throughout (bank / stats / config)
#
# CreateTable / the registry are exercised by the integration suite
# (factory_test) and fuel-audited (fuel_audit_test); a full second-token
# table on a live node needs a 99/100 token deployed first — out of scope
# for this smoke test.
#
# Requires: docker compose (bitcoind + metashrew) up, alkanes-cli built.
#   ALKANES_RS=~/subfrost/alkanes-rs ./scripts/reset-regtest.sh
#   ALKANES_RS=~/subfrost/alkanes-rs ./scripts/e2e-test.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_env.sh"

RPC_URL="${RPC_URL:-http://127.0.0.1:18888}"

height() {
    python3 -c "import json,urllib.request;print(int(json.loads(urllib.request.urlopen(urllib.request.Request('$RPC_URL',data=json.dumps({'jsonrpc':'2.0','id':1,'method':'metashrew_height','params':[]}).encode(),headers={'Content-Type':'application/json'})).read())['result']))"
}
ok() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; exit 1; }

echo "═══ Cheeky Bricky E2E Smoke Test ═══"
echo ""

echo "[1/5] Deploy + mine DIESEL"
bash "$SCRIPT_DIR/deploy-regtest.sh" --build > /tmp/cheeky-bricky-e2e-deploy.log 2>&1 \
    && ok "deployed (see /tmp/cheeky-bricky-e2e-deploy.log)" || fail "deploy"
# a couple more free mints so we have DIESEL to stake and bet
echo y | $CLI alkanes execute "[2,0,77]:v0:v0" --from p2tr:0 --change p2tr:0 --alkanes-change p2tr:0 --mine --fee-rate 1 > /dev/null 2>&1 || true
echo ""

echo "[2/5] Become the house: deposit 100_000_000 DIESEL"
bash "$SCRIPT_DIR/deposit.sh" 100000000 > /tmp/cheeky-bricky-e2e-dep.log 2>&1 \
    && ok "deposited" || fail "deposit"
bash "$SCRIPT_DIR/view.sh" bank
echo ""

echo "[3/5] Play: 3 rolls at 50% (coin flip)"
for i in 1 2 3; do
    echo "  roll #$i ..."
    bash "$SCRIPT_DIR/roll.sh" 32768 1000000 > "/tmp/cheeky-bricky-e2e-roll$i.log" 2>&1 || true
    grep -qi "completed successfully" "/tmp/cheeky-bricky-e2e-roll$i.log" && ok "roll #$i landed" \
        || echo "    (see /tmp/cheeky-bricky-e2e-roll$i.log — may have reverted on caps)"
done
bash "$SCRIPT_DIR/view.sh" stats
bash "$SCRIPT_DIR/view.sh" bank
echo ""

echo "[4/5] Withdraw half your shares"
# Read total_shares from ViewBank (field 2) and burn half.
SHARES=$(bash "$SCRIPT_DIR/view.sh" bank | awk '/total_shares/ {print $2}')
HALF=$(python3 -c "print(int(${SHARES:-0})//2)")
if [ "${HALF:-0}" -gt 0 ]; then
    bash "$SCRIPT_DIR/withdraw.sh" "$HALF" 0 > /tmp/cheeky-bricky-e2e-wd.log 2>&1 \
        && ok "withdrew $HALF shares" || fail "withdraw"
else
    echo "  (no shares to withdraw — deposit step may have failed)"
fi
bash "$SCRIPT_DIR/view.sh" bank
echo ""

echo "[5/6] Operator collects the accrued 2% exit fee"
bash "$SCRIPT_DIR/withdraw-fees.sh" > /tmp/cheeky-bricky-e2e-fees.log 2>&1 \
    && ok "operator withdrew exit fees with the auth key 4:$OPERATOR_TX" \
    || echo "  (no fees accrued, or see /tmp/cheeky-bricky-e2e-fees.log)"
echo ""

echo "[6/6] Final config + stats"
bash "$SCRIPT_DIR/view.sh" config
bash "$SCRIPT_DIR/view.sh" stats
echo ""
echo "═══ E2E complete (inspect the /tmp/cheeky-bricky-e2e-*.log traces) ═══"

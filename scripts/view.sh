#!/usr/bin/env bash
# Query Dice state via direct `metashrew_view` JSON-RPC (no wallet
# provider — bypasses regtest protorunesbyaddress flakiness). Decoder
# pattern lifted from arbuzino/scripts/view.sh, incl. the proper protobuf
# varint parsing (single-byte length reads corrupt responses ≥ 128 bytes).
#
# Usage:
#   ./scripts/view.sh bank              # (bank, total_shares, fee_accum, exit_bps)
#   ./scripts/view.sh stats             # (total_bets, total_wagered, total_won)
#   ./scripts/view.sh config            # (min_bet, edge, t_min, t_max, token, exit_bps_max)
#   ./scripts/view.sh max-bet <target>  # (max_bet, max_net_profit) at current bank
#   ./scripts/view.sh table-of <b> <t>  # canonical table for token b:t (zeros = none)
#   ./scripts/view.sh exit-fee          # (fee_now, fee_next, activation_height, delay)

set -euo pipefail

if [ $# -lt 1 ]; then
    echo "Usage: $0 <bank|stats|config|exit-fee|max-bet <target>|table-of <b> <t>>"
    exit 1
fi

GAME_BLOCK=4
GAME_TX=64206
RPC_URL="${RPC_URL:-http://127.0.0.1:18888}"

ARGS=""
case "$1" in
    bank)   OPCODE=101 ;;
    stats)  OPCODE=102 ;;
    config) OPCODE=103 ;;
    exit-fee) OPCODE=106 ;;
    max-bet)
        if [ $# -lt 2 ]; then
            echo "Usage: $0 max-bet <target>"
            exit 1
        fi
        ARGS="$2"
        OPCODE=104
        ;;
    table-of)
        if [ $# -lt 3 ]; then
            echo "Usage: $0 table-of <token_block> <token_tx>"
            exit 1
        fi
        ARGS="$2 $3"
        OPCODE=105
        ;;
    *) echo "Unknown view: $1"; exit 1 ;;
esac

python3 - <<PYEOF
import json, urllib.request

def leb128(n):
    out = []
    while True:
        b = n & 0x7f
        n >>= 7
        if n:
            b |= 0x80
        out.append(b)
        if not n:
            break
    return out

def rpc(method, params):
    req = urllib.request.Request(
        "$RPC_URL",
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        headers={"Content-Type": "application/json"},
    )
    return json.loads(urllib.request.urlopen(req).read())

height = int(rpc("metashrew_height", [])["result"])

cd = leb128($GAME_BLOCK) + leb128($GAME_TX) + leb128($OPCODE)
for a in "$ARGS".split():
    cd += leb128(int(a))
parts = [0x20] + leb128(height) + [0x2a, len(cd)] + cd + [0x30, 0x01]
hx = "0x" + bytes(parts).hex()

r = rpc("metashrew_view", ["simulate", hx, "latest"])
result = r.get("result", "0x")
if not result or result == "0x":
    print(f"empty response (height={height})")
    raise SystemExit(0)

data = bytes.fromhex(result[2:])

def read_varint(b, i):
    val = 0
    shift = 0
    while True:
        byte = b[i]
        i += 1
        val |= (byte & 0x7f) << shift
        if (byte & 0x80) == 0:
            return val, i
        shift += 7

def read_field(b, i):
    tag, i = read_varint(b, i)
    wire = tag & 0x7
    fnum = tag >> 3
    if wire == 0:
        val, i = read_varint(b, i)
        return fnum, val, i
    elif wire == 2:
        ln, i = read_varint(b, i)
        return fnum, b[i:i+ln], i + ln
    else:
        raise ValueError(f"unsupported wire type {wire}")

ret = None
i = 0
while i < len(data):
    fnum, val, ni = read_field(data, i)
    if fnum == 1 and isinstance(val, (bytes, bytearray)):
        j = 0
        while j < len(val):
            inner_fnum, inner_val, j = read_field(val, j)
            if inner_fnum == 3 and isinstance(inner_val, (bytes, bytearray)):
                ret = inner_val
                break
        if ret is not None:
            break
    i = ni

if ret is None:
    print(f"no return_data found in: {result}")
    raise SystemExit(0)

print(f"raw {len(ret)} bytes: {ret.hex()}")

def u128_le(b):
    if len(b) < 16:
        return None
    return int.from_bytes(b[:16], "little")

LABELS = {
    "bank":    ["bank", "total_shares", "fee_accum", "exit_bps"],
    "stats":   ["total_bets", "total_wagered", "total_won"],
    "config":  ["min_bet", "edge_bps", "t_min", "t_max", "token_block", "token_tx", "exit_bps_max"],
    "max-bet": ["max_bet", "max_net_profit"],
    "table-of": ["table_block", "table_tx"],
    "exit-fee": ["fee_now", "fee_next", "activation_height", "delay_blocks"],
}
for i, lbl in enumerate(LABELS["$1"]):
    v = u128_le(ret[i*16:(i+1)*16])
    print(f"  {lbl}: {v}")
PYEOF

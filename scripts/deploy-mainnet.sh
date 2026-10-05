#!/usr/bin/env bash
# Dice — MAINNET deployment: the game, then the operator auth key.
#
# Deploys the two production contracts, operator LAST (owner's call, so no
# deploy tx spends this wallet's UTXOs after the keys are minted):
#   1. dice-game at 4:$GAME_TX (default 0xFACE = 64206), init with the bet
#      token (2:69, FARTANE / H2S); its shares keep the default "Dice"/"DICE".
#      The template is also the canonical table registry and the H2S table.
#   2. dice-operator at 4:<OPERATOR_AUTH_ID.tx> (0xDEAD = 57005) — its init
#      mints the three fee-withdrawal keys to this wallet. The game hardcodes
#      this id, so it is read from the runtime source, never typed twice.
# The operator goes out only after the game is confirmed and verified. The
# game trusts 4:0xDEAD from its first block, so the slot is re-checked right
# before the operator broadcast; whoever held that slot would hold the fee key
# (fees and the 0..2% exit fee — never LP or player funds). A reserved slot
# can't be overwritten (CREATERESERVED onto an occupied one reverts), so a
# clean trace of OUR operator deploy proves the key contract is ours.
#
# Hardened:
#   - FEE_RATE and DEPLOY_PASSWORD required (no silent defaults); the game
#     slot is the agreed 4:64206 unless GAME_TX overrides it; no --mine on
#     mainnet (wait_for_deploy polls traces)
#   - builds the wasm itself, from a clean committed tree, with the pinned
#     toolchain, and refuses bytes that embed local paths — the sha256 of each
#     deployed wasm lands in the deployment record, so anyone can rebuild it
#   - pre-flight: wallet + funds, bet token answers GetName, both slots free
#   - cost estimate + a typed confirmation before EACH broadcast
#   - resumable: a txid is recorded the moment it's known; a re-run verifies
#     what's already deployed instead of deploying it again
#   - post-deploy checks by simulate: names, ViewConfig, ViewTableOf, ViewBank
#     (the init params are immutable, so they're compared exactly)
#
# Usage:
#   # 1) preflight only — builds, checks wallet/funds/token/slots, prints the
#   #    exact commands and the cost; broadcasts nothing:
#   DEPLOY_PASSWORD=... FEE_RATE=3 PREFLIGHT_ONLY=1 ./scripts/deploy-mainnet.sh
#
#   # 2) real deploy (asks you to type "mainnet" before each broadcast):
#   DEPLOY_PASSWORD=... FEE_RATE=3 ./scripts/deploy-mainnet.sh
#
#   # rehearsal on local regtest (auto-mines; start from reset-regtest.sh):
#   NETWORK=regtest DEPLOY_PASSWORD=testtest FEE_RATE=1 ./scripts/deploy-mainnet.sh
#
# Optional env: GAME_TX, RPC_URL, ESPLORA_URL, WALLET_FILE, BET_TOKEN (block:tx),
# OPERATOR_TXID / GAME_TXID (resume from a known reveal txid), TX_TIMEOUT
# (seconds to wait for a confirmation), ASSUME_YES=1 (skip the typed gates).
# alkanes-cli takes the passphrase as an argument, so it is visible in the
# local process list while a command runs.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WASM_DIR="$PROJECT_DIR/target/wasm32-unknown-unknown/release"
OPERATOR_WASM="$WASM_DIR/dice_operator.wasm"
GAME_WASM="$WASM_DIR/dice_game.wasm"
RUNTIME_SRC="$PROJECT_DIR/alkanes/dice-game-runtime/src/lib.rs"
BUILD_LOG="$PROJECT_DIR/target/deploy-build.log"

: "${DEPLOY_PASSWORD:?ERROR: DEPLOY_PASSWORD is required}"
: "${FEE_RATE:?ERROR: FEE_RATE is required (sats/vbyte, e.g. 3)}"
GAME_TX="${GAME_TX:-64206}"   # 0xFACE: the game's reserved slot, 4:64206

NETWORK="${NETWORK:-mainnet}"
PREFLIGHT_ONLY="${PREFLIGHT_ONLY:-0}"
ASSUME_YES="${ASSUME_YES:-0}"
case "$NETWORK" in
    mainnet)
        RPC_URL="${RPC_URL:-https://mainnet.subfrost.io/v4/subfrost}"
        # blockstream, not mempool.space: alkanes-cli (both the develop build and
        # the release used for the mainnet deploy) fails to decode mempool.space's UTXO
        # responses ("error decoding response body", 2026-10-01)
        ESPLORA_URL="${ESPLORA_URL:-https://blockstream.info/api}"
        WALLET_FILE="${WALLET_FILE:-$HOME/.alkanes/wallet-dice-mainnet.json}"
        BET_TOKEN="${BET_TOKEN:-2:69}"            # FARTANE, symbol H2S
        STATE_FILE="$PROJECT_DIR/deployments/mainnet.env"
        TX_TIMEOUT="${TX_TIMEOUT:-7200}"
        ;;
    regtest)
        RPC_URL="${RPC_URL:-http://127.0.0.1:18888}"
        ESPLORA_URL="${ESPLORA_URL:-}"
        WALLET_FILE="${WALLET_FILE:-$HOME/.alkanes/wallet-dice.json}"
        BET_TOKEN="${BET_TOKEN:-2:0}"
        STATE_FILE="$PROJECT_DIR/target/deploy-regtest.env"
        TX_TIMEOUT="${TX_TIMEOUT:-600}"
        # bulk mining goes to bitcoind directly: alkanes-jsonrpc caps
        # generatetoaddress at 1 block per call
        BITCOIN_RPC_URL="${BITCOIN_RPC_URL:-http://bitcoinrpc:bitcoinrpc@127.0.0.1:18443}"
        ;;
    *) echo "ERROR: NETWORK must be mainnet or regtest"; exit 1 ;;
esac

[[ "$FEE_RATE" =~ ^[0-9]+([.][0-9]+)?$ ]] || { echo "ERROR: FEE_RATE must be a number"; exit 1; }
[[ "$GAME_TX" =~ ^[0-9]+$ ]] || { echo "ERROR: GAME_TX must be an integer"; exit 1; }
[[ "$BET_TOKEN" =~ ^[0-9]+:[0-9]+$ ]] || { echo "ERROR: BET_TOKEN must be block:tx"; exit 1; }
BET_B="${BET_TOKEN%%:*}"
BET_T="${BET_TOKEN##*:}"

# The operator slot comes from the game's own constant, so the two can't drift.
OP_LINE="$(grep -E 'pub const OPERATOR_AUTH_ID: AlkaneId = AlkaneId \{ block: [0-9]+, tx: (0x[0-9A-Fa-f]+|[0-9]+) \};' "$RUNTIME_SRC" || true)"
[ -n "$OP_LINE" ] || { echo "ERROR: can't read OPERATOR_AUTH_ID from $RUNTIME_SRC"; exit 1; }
OPERATOR_BLOCK="$(echo "$OP_LINE" | sed -E 's/.*block: ([0-9]+),.*/\1/')"
OPERATOR_TX="$(( $(echo "$OP_LINE" | sed -E 's/.*tx: (0x[0-9A-Fa-f]+|[0-9]+) .*/\1/') ))"
[ "$OPERATOR_BLOCK" = "4" ] || { echo "ERROR: OPERATOR_AUTH_ID must be a reserved 4:N id"; exit 1; }
[ "$GAME_TX" != "$OPERATOR_TX" ] || { echo "ERROR: GAME_TX can't be the operator slot $OPERATOR_TX"; exit 1; }
KEY_SUPPLY="$(sed -nE 's/^pub const KEY_SUPPLY: u128 = ([0-9]+);.*/\1/p' "$PROJECT_DIR/alkanes/dice-operator/src/lib.rs")"
EXIT_FEE_DELAY="$(sed -nE 's/^pub const EXIT_FEE_DELAY: u128 = ([0-9]+);.*/\1/p' "$RUNTIME_SRC")"
[ -n "$EXIT_FEE_DELAY" ] || { echo "ERROR: can't read EXIT_FEE_DELAY from $RUNTIME_SRC"; exit 1; }
[ -n "$KEY_SUPPLY" ] || { echo "ERROR: can't read KEY_SUPPLY from alkanes/dice-operator/src/lib.rs"; exit 1; }

ALKANES_RS="${ALKANES_RS:-$HOME/subfrost/alkanes-rs}"
ALKANES_CLI="${ALKANES_CLI:-}"
if [ -z "$ALKANES_CLI" ]; then
    for candidate in \
        "$ALKANES_RS/target/release/alkanes-cli" \
        "$(which alkanes-cli 2>/dev/null || true)"; do
        [ -n "$candidate" ] && [ -x "$candidate" ] && ALKANES_CLI="$candidate" && break
    done
fi
[ -n "$ALKANES_CLI" ] || { echo "ERROR: alkanes-cli not found (set ALKANES_CLI= or ALKANES_RS=)"; exit 1; }

# alkanes-cli (develop) caches getbytecode/meta results FOREVER in
# ~/.alkanes/cache.sqlite3, keyed by network, not by chain: after a regtest reset
# it reports a stale contract, and a slot read as empty once can stay "empty".
# Every run gets an empty cache of its own; the shared one is left alone.
ALKANES_CACHE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/dice-deploy-cache.XXXXXX")"
export ALKANES_CACHE_DIR
trap 'rm -rf "$ALKANES_CACHE_DIR"' EXIT

CLI=(
    "$ALKANES_CLI"
    -p "$NETWORK"
    --jsonrpc-url "$RPC_URL"
    --wallet-file "$WALLET_FILE"
    --passphrase "$DEPLOY_PASSWORD"
)
[ -n "$ESPLORA_URL" ] && CLI+=( --esplora-api-url "$ESPLORA_URL" )

log()       { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
log_err()   { printf '\033[31m[ERROR]\033[0m %s\n' "$*" >&2; }
log_ok()    { printf '\033[32m[OK]\033[0m %s\n' "$*"; }
log_warn()  { printf '\033[33m[WARN]\033[0m %s\n' "$*"; }
log_phase() { printf '\n\033[1m═══ %s ═══\033[0m\n' "$*"; }
die()       { log_err "$*"; exit 2; }

# ── Views: metashrew_view "simulate" straight over JSON-RPC ────────
# Prints DATA:<hex> (the call's return data) or ERROR:<message>. Request
# encoding as in view.sh: MessageContextParcel{height=4, calldata=5, txindex=6};
# response SimulateResponse{execution=1 (data=3), gas_used=2, error=3}.
sim() {  # block tx opcode [args...]
    python3 - "$RPC_URL" "$@" <<'PY'
import json, sys, urllib.request

url, nums = sys.argv[1], [int(a) for a in sys.argv[2:]]

def leb128(n):
    out = bytearray()
    while True:
        b, n = n & 0x7F, n >> 7
        out.append(b | 0x80 if n else b)
        if not n:
            return bytes(out)

def rpc(method, params):
    req = urllib.request.Request(
        url,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "dice-deploy"},
    )
    r = json.loads(urllib.request.urlopen(req, timeout=30).read())
    if r.get("error"):
        raise RuntimeError(r["error"])
    return r["result"]

def varint(b, i):
    v = s = 0
    while True:
        x = b[i]
        i += 1
        v |= (x & 0x7F) << s
        if not x & 0x80:
            return v, i
        s += 7

def fields(b):
    i = 0
    while i < len(b):
        tag, i = varint(b, i)
        if tag & 7 == 0:
            v, i = varint(b, i)
        elif tag & 7 == 2:
            n, i = varint(b, i)
            v, i = b[i:i + n], i + n
        else:
            raise ValueError("unsupported wire type %d" % (tag & 7))
        yield tag >> 3, v

try:
    height = int(rpc("metashrew_height", []))
    cd = b"".join(leb128(n) for n in nums)
    parcel = b"\x20" + leb128(height) + b"\x2a" + leb128(len(cd)) + cd + b"\x30\x01"
    res = rpc("metashrew_view", ["simulate", "0x" + parcel.hex(), "latest"]) or "0x"
    data, err = b"", ""
    for f, v in fields(bytes.fromhex(res[2:])):
        if f == 1 and isinstance(v, bytes):
            for g, w in fields(v):
                if g == 3 and isinstance(w, bytes):
                    data = w
        elif f == 3 and isinstance(v, bytes):
            err = v.decode("utf-8", "replace")
    print("ERROR:" + err.replace("\n", " ") if err else "DATA:" + data.hex())
except Exception as e:
    print("ERROR:" + str(e).replace("\n", " "))
PY
}
indexer_height() {
    python3 - "$RPC_URL" <<'PY'
import json, sys, urllib.request
req = urllib.request.Request(
    sys.argv[1],
    data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": "metashrew_height", "params": []}).encode(),
    headers={"Content-Type": "application/json", "User-Agent": "dice-deploy"},
)
try:
    print(int(json.loads(urllib.request.urlopen(req, timeout=10).read())["result"]))
except Exception:
    print(-1)
PY
}
sim_data()   { local r; r="$(sim "$@")"; if [ "${r#DATA:}" != "$r" ]; then echo "${r#DATA:}"; fi; }
sim_string() { local h; h="$(sim_data "$@")"; [ -z "$h" ] || python3 -c 'import sys; print(bytes.fromhex(sys.argv[1]).decode("utf-8", "replace"))' "$h"; }
u128s_hex()  { python3 -c 'import sys; print("".join(int(a).to_bytes(16, "little").hex() for a in sys.argv[1:]))' "$@"; }
str_hex()    { python3 -c 'import sys; print(sys.argv[1].encode().hex())' "$1"; }

VERIFY_FAILED=0
expect_data() {  # label expected_hex block tx opcode [args...]
    local label="$1" want="$2" got
    shift 2
    got="$(sim "$@")"
    if [ "$got" = "DATA:$want" ]; then
        log_ok "$label"
    else
        log_err "$label — got ${got:0:240}"
        VERIFY_FAILED=1
    fi
}

# Occupied = has bytecode, or answers GetName (getbytecode can read empty for
# a fresh CREATERESERVED deploy — observed on subfrost mainnet).
slot_occupied() {  # block tx
    local bc
    bc="$(RUST_LOG=off "${CLI[@]}" alkanes getbytecode "$1:$2" 2>/dev/null | grep -oE '0x[0-9a-fA-F]+' | head -1 || true)"
    [ "${#bc}" -gt 20 ] && return 0
    [ -n "$(sim_data "$1" "$2" 99)" ] && return 0
    return 1
}

est_sats() {  # wasm_path → commit+reveal estimate (the wasm rides the witness)
    python3 -c 'import sys, math; print(math.ceil((int(sys.argv[1]) // 4 + 600) * float(sys.argv[2])))' \
        "$(wc -c < "$1")" "$FEE_RATE"
}

# ── Deployment record (also the resume state) ──────────────────────
state_get() { if [ -f "$STATE_FILE" ]; then sed -n "s/^$1=\"\{0,1\}\([^\"]*\)\"\{0,1\}$/\1/p" "$STATE_FILE" | tail -1; fi; }
OPERATOR_TXID="${OPERATOR_TXID:-$(state_get OPERATOR_TXID)}"
GAME_TXID="${GAME_TXID:-$(state_get GAME_TXID)}"
COMMIT="" RUSTC_VERSION="" CLANG_VERSION="" LOCK_SHA="" OPERATOR_SHA="" GAME_SHA=""

write_state() {
    mkdir -p "$(dirname "$STATE_FILE")"
    cat > "$STATE_FILE" <<EOF
# Dice $NETWORK deployment — written by scripts/deploy-mainnet.sh,
# updated $(date -u +%Y-%m-%dT%H:%M:%SZ). Commit this file after a mainnet deploy.
# Rebuild check: git checkout \$COMMIT && ./scripts/build-wasms.sh && shasum -a 256 \\
#   target/wasm32-unknown-unknown/release/dice_{operator,game}.wasm
NETWORK="$NETWORK"
COMMIT="$COMMIT"
RUSTC="$RUSTC_VERSION"
CLANG="$CLANG_VERSION"
CARGO_LOCK_SHA256="$LOCK_SHA"
OPERATOR_WASM_SHA256="$OPERATOR_SHA"
GAME_WASM_SHA256="$GAME_SHA"
OPERATOR_ID="4:$OPERATOR_TX"
OPERATOR_TXID="$OPERATOR_TXID"
GAME_ID="4:$GAME_TX"
GAME_TXID="$GAME_TXID"
BET_TOKEN="$BET_TOKEN"
FEE_RATE="$FEE_RATE"
EOF
}

# ── Broadcast + confirmation ───────────────────────────────────────
confirm() {
    [ "$ASSUME_YES" = "1" ] && return 0
    local resp
    read -rp "Broadcast $1 to $NETWORK? Type '$NETWORK' to confirm: " resp
    [ "$resp" = "$NETWORK" ] || { log_warn "aborted — nothing broadcast"; exit 1; }
}

LAST_TXID=""
broadcast() {  # cellpack wasm label
    local -a args=(
        alkanes execute "$1"
        --envelope "$2"
        --from p2tr:0
        --change p2tr:0
        --alkanes-change p2tr:0
        --fee-rate "$FEE_RATE"
    )
    [ "$NETWORK" = "regtest" ] && args+=( --mine )
    log "▶ $3"
    local out
    out="$(echo y | "${CLI[@]}" "${args[@]}" 2>&1)" || {
        log_err "$3: alkanes-cli failed"; echo "$out" | tail -15 >&2; exit 3
    }
    LAST_TXID="$(echo "$out" | grep -oE '(Reveal TXID|TXID): [0-9a-f]{64}' | awk '{print $NF}' | tail -1 || true)"
    [ -n "$LAST_TXID" ] || { log_err "$3: no txid in the alkanes-cli output"; echo "$out" | tail -15 >&2; exit 3; }
    log_ok "$3 broadcast, txid $LAST_TXID"
}

DEPLOY_TRACE=""
wait_for_deploy() {  # txid block tx — until the trace that creates block:tx shows up
    # Find OUR protostone by what it did (create_alkane block:tx), not by vout:
    # newer alkanes-cli appends a DIESEL-mint protostone [2,0,77] to every tx,
    # and on mainnet that mint can revert (block already claimed) — that must
    # not read as our deploy failing, nor shift which vout we inspect.
    local txid="$1" want_block="$2" want_tx="$3" start vout trace indexed
    start=$(date +%s)
    log "⏳ waiting for $txid to confirm and index (timeout ${TX_TIMEOUT}s)..."
    while true; do
        indexed=0
        for vout in 2 3 4 5 6 7 8; do
            trace="$("${CLI[@]}" alkanes trace "${txid}:${vout}" 2>&1 || true)"
            # here-strings, not `echo | grep -q`: under pipefail an early grep
            # exit SIGPIPEs the echo and a real trace would read as "not yet"
            grep -qE "(ALKANES EXECUTION TRACE|total_events)" <<< "$trace" || continue
            indexed=1
            if grep -q "create_alkane" <<< "$trace" && grep -qE "^[[:space:]│]*tx: ${want_tx}\$" <<< "$trace"; then
                if grep -qE "(❌ revert|error_data:|status: ❌)" <<< "$trace"; then
                    die "deploy of $want_block:$want_tx REVERTED (inspect: alkanes trace ${txid}:${vout})"
                fi
                log_ok "confirmed: $want_block:$want_tx created (trace at vout $vout)"
                DEPLOY_TRACE="$trace"
                return 0
            fi
        done
        if (( indexed )); then
            die "tx $txid is indexed but none of its protostones created $want_block:$want_tx (inspect: alkanes trace ${txid}:<vout>)"
        fi
        if (( $(date +%s) - start > TX_TIMEOUT )); then
            die "timed out waiting for $txid — re-run later to resume (the txid is in $STATE_FILE)"
        fi
        sleep 10
    done
}

# ── Pre-flight ─────────────────────────────────────────────────────
preflight() {
    log_phase "Pre-flight"
    command -v python3 >/dev/null || die "python3 required"

    COMMIT="$(git -C "$PROJECT_DIR" rev-parse HEAD)"
    local dirty
    dirty="$(git -C "$PROJECT_DIR" status --porcelain -- . ':(exclude)deployments')"
    if [ -n "$dirty" ]; then
        [ "$NETWORK" = "mainnet" ] && die "uncommitted changes — mainnet deploys only committed code:
$dirty"
        log_warn "uncommitted changes (fine for $NETWORK)"
    fi
    if [ -z "$(git -C "$PROJECT_DIR" branch -r --contains HEAD 2>/dev/null)" ]; then
        log_warn "HEAD ${COMMIT:0:12} is on no remote branch — push it so the deployed wasm can be rebuilt by others"
    fi
    local recorded
    recorded="$(state_get COMMIT)"
    if [ -n "$recorded" ] && [ "$recorded" != "$COMMIT" ] && [ -n "$OPERATOR_TXID$GAME_TXID" ]; then
        [ "$NETWORK" = "mainnet" ] && die "this deployment was started at commit $recorded but HEAD is $COMMIT — check out $recorded to finish it from the same sources"
        log_warn "state recorded at $recorded, HEAD is $COMMIT"
    fi
    log_ok "sources: commit ${COMMIT:0:12}"

    # The pin only works if nothing overrides it (RUSTUP_TOOLCHAIN, +toolchain).
    local pinned active
    pinned="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$PROJECT_DIR/rust-toolchain.toml")"
    active="$(cd "$PROJECT_DIR" && rustup show active-toolchain | awk '{print $1}')"
    case "$active" in
        "$pinned"-*) ;;
        *) die "active toolchain $active is not the pinned $pinned (unset RUSTUP_TOOLCHAIN?)" ;;
    esac
    RUSTC_VERSION="$(cd "$PROJECT_DIR" && rustc --version)"
    # the C compiler build-wasms.sh picks: $CC, else Homebrew LLVM, else clang
    local cc="${CC:-}"
    [ -n "$cc" ] || { [ -x /opt/homebrew/opt/llvm/bin/clang ] && cc=/opt/homebrew/opt/llvm/bin/clang; } || cc=clang
    CLANG_VERSION="$("$cc" --version 2>/dev/null | head -1 || echo unknown)"

    log "building wasm with $RUSTC_VERSION (log: $BUILD_LOG)..."
    mkdir -p "$(dirname "$BUILD_LOG")"
    bash "$SCRIPT_DIR/build-wasms.sh" > "$BUILD_LOG" 2>&1 || { tail -20 "$BUILD_LOG" >&2; die "build failed"; }
    local w
    for w in "$OPERATOR_WASM" "$GAME_WASM"; do
        [ -f "$w" ] || die "missing $w"
        LC_ALL=C grep -aq "$HOME" "$w" && die "$(basename "$w") embeds local paths ($HOME) — not reproducible, refusing to deploy"
    done
    OPERATOR_SHA="$(shasum -a 256 "$OPERATOR_WASM" | awk '{print $1}')"
    GAME_SHA="$(shasum -a 256 "$GAME_WASM" | awk '{print $1}')"
    LOCK_SHA="$(shasum -a 256 "$PROJECT_DIR/Cargo.lock" | awk '{print $1}')"
    log_ok "operator wasm $(wc -c < "$OPERATOR_WASM" | tr -d ' ') bytes, sha256 $OPERATOR_SHA"
    log_ok "game wasm     $(wc -c < "$GAME_WASM" | tr -d ' ') bytes, sha256 $GAME_SHA"

    if [ "$NETWORK" = "regtest" ]; then
        log "regtest: wallet + 301 blocks of coinbase..."
        local h0 waited=0
        h0="$(indexer_height)"
        (( h0 >= 0 )) || die "regtest RPC $RPC_URL unreachable (start it: reset-regtest.sh)"
        if [ ! -f "$WALLET_FILE" ]; then
            "${CLI[@]}" wallet create >/dev/null 2>&1 || die "can't create the regtest wallet"
        fi
        "${CLI[@]}" --bitcoin-rpc-url "$BITCOIN_RPC_URL" bitcoind generatetoaddress 301 "p2tr:0" >/dev/null 2>&1 \
            || die "can't mine on regtest (bitcoind at $BITCOIN_RPC_URL)"
        until (( $(indexer_height) >= h0 + 301 )); do
            (( waited++ < 300 )) || die "regtest indexer didn't catch up (at $(indexer_height), want $(( h0 + 301 )))"
            sleep 1
        done
    fi

    log "checking RPC + wallet ($WALLET_FILE)..."
    local wout
    wout="$("${CLI[@]}" wallet balance 2>&1)" \
        || die "wallet balance failed: $(grep -m1 -E 'Error|error' <<< "$wout" | cut -c1-200) (wallet file, RPC_URL or ESPLORA_URL)"
    local need=0 have utxos
    [ -z "$OPERATOR_TXID" ] && need=$(( need + $(est_sats "$OPERATOR_WASM") ))
    [ -z "$GAME_TXID" ] && need=$(( need + $(est_sats "$GAME_WASM") ))
    utxos="$("${CLI[@]}" wallet utxos 2>&1 || true)"
    have="$(grep -oE 'Amount \(sats\): [0-9]+' <<< "$utxos" | awk '{s += $3} END {print s + 0}' || true)"
    have="${have:-0}"
    (( have * 2 >= need * 3 )) || die "wallet has $have sats; the pending deploys need ~$need (+50% margin)"
    log_ok "wallet has $have sats (pending deploys ~$need)"

    BET_NAME="$(sim_string "$BET_B" "$BET_T" 99)"
    BET_SYMBOL="$(sim_string "$BET_B" "$BET_T" 100)"
    if [ -n "$BET_NAME" ]; then
        log_ok "bet token $BET_TOKEN = \"$BET_NAME\" / \"$BET_SYMBOL\""
    elif [ "$NETWORK" = "mainnet" ]; then
        die "bet token $BET_TOKEN doesn't answer GetName (99): $(sim "$BET_B" "$BET_T" 99)"
    else
        log_warn "bet token $BET_TOKEN doesn't answer GetName (99)"
    fi
    [ "$NETWORK" = "mainnet" ] && [ "$BET_TOKEN" != "2:69" ] && log_warn "BET_TOKEN is $BET_TOKEN, NOT the documented mainnet 2:69 (H2S) — the bet token is immutable!"

    if [ -n "$OPERATOR_TXID" ]; then
        log_ok "operator: recorded as broadcast (txid $OPERATOR_TXID) — will verify, not redeploy"
    elif slot_occupied "$OPERATOR_BLOCK" "$OPERATOR_TX"; then
        die "4:$OPERATOR_TX (OPERATOR_AUTH_ID) is already occupied and this deployment has no record of creating it.
  Deployed it yourself? Re-run with OPERATOR_TXID=<its reveal txid>.
  Otherwise move to a free reserved id: change OPERATOR_AUTH_ID in
  $RUNTIME_SRC, run the tests, commit, and run this again."
    else
        log_ok "operator slot 4:$OPERATOR_TX is free"
    fi
    if [ -n "$GAME_TXID" ]; then
        log_ok "game: recorded as broadcast (txid $GAME_TXID) — will verify, not redeploy"
    elif slot_occupied 4 "$GAME_TX"; then
        die "4:$GAME_TX is already occupied — pick another GAME_TX"
    else
        log_ok "game slot 4:$GAME_TX is free"
    fi
}

verify_operator() {
    log_phase "Verify operator 4:$OPERATOR_TX"
    expect_data 'GetName = "Dice Operator"' "$(str_hex 'Dice Operator')" "$OPERATOR_BLOCK" "$OPERATOR_TX" 99
    expect_data 'GetSymbol = "DOP"' "$(str_hex DOP)" "$OPERATOR_BLOCK" "$OPERATOR_TX" 100
    # The keys, from the deploy trace: Init's return goes to pointer v0, this
    # wallet's p2tr:0 (address-balance APIs lag, and are absent on regtest).
    local transfers
    transfers="$(grep -A8 'alkane_transfers' <<< "$DEPLOY_TRACE" || true)"
    if grep -qE "tx: ${OPERATOR_TX}\$" <<< "$transfers" && grep -qE "amount: ${KEY_SUPPLY}\$" <<< "$transfers"; then
        log_ok "Init handed $KEY_SUPPLY key units of 4:$OPERATOR_TX to this wallet's p2tr:0"
    else
        log_err "the deploy trace doesn't show $KEY_SUPPLY keys of 4:$OPERATOR_TX handed out"
        VERIFY_FAILED=1
    fi
}

verify_game() {  # fresh=1 right after this run deployed it
    log_phase "Verify game 4:$GAME_TX"
    expect_data 'GetName = "Dice"' "$(str_hex 'Dice')" 4 "$GAME_TX" 99
    expect_data 'GetSymbol = "DICE"' "$(str_hex DICE)" 4 "$GAME_TX" 100
    # Launch config as documented in CLAUDE.md (min_bet, edge, t_min, t_max,
    # bet token, exit-fee ceiling) — update deliberately if a constant changes.
    expect_data "ViewConfig = (1000, 100, 1, 64238, $BET_B, $BET_T, 200)" \
        "$(u128s_hex 1000 100 1 64238 "$BET_B" "$BET_T" 200)" 4 "$GAME_TX" 103
    expect_data "ViewTableOf($BET_TOKEN) = 4:$GAME_TX" "$(u128s_hex 4 "$GAME_TX")" 4 "$GAME_TX" 105 "$BET_B" "$BET_T"
    if [ "$1" = "1" ]; then
        expect_data "ViewBank = (0, 0, 0, 200): empty bank, 2% exit fee" "$(u128s_hex 0 0 0 200)" 4 "$GAME_TX" 101
        expect_data "ViewExitFee = (200, 200, 0, $EXIT_FEE_DELAY): no change scheduled, ${EXIT_FEE_DELAY}-block delay" \
            "$(u128s_hex 200 200 0 "$EXIT_FEE_DELAY")" 4 "$GAME_TX" 106
    fi
}

main() {
    echo "═══════════════════════════════════════════════"
    echo "  Dice — $NETWORK deploy"
    echo "  RPC:        $RPC_URL"
    echo "  Wallet:     $WALLET_FILE"
    echo "  Fee rate:   $FEE_RATE sats/vbyte"
    echo "  Operator:   4:$OPERATOR_TX (OPERATOR_AUTH_ID)"
    echo "  Game:       4:$GAME_TX, bet token $BET_TOKEN"
    echo "  Record:     $STATE_FILE"
    echo "═══════════════════════════════════════════════"

    preflight

    local op_cellpack="[3,$OPERATOR_TX,0]:v0:v0"
    local game_cellpack="[3,$GAME_TX,0,$BET_B,$BET_T,0,0]:v0:v0"
    log_phase "Plan"
    [ -z "$GAME_TXID" ] && log "1. game:     alkanes execute \"$game_cellpack\" --envelope dice_game.wasm  (~$(est_sats "$GAME_WASM") sats)"
    [ -z "$OPERATOR_TXID" ] && log "2. operator: alkanes execute \"$op_cellpack\" --envelope dice_operator.wasm  (~$(est_sats "$OPERATOR_WASM") sats)"
    log "   (--from p2tr:0 --change p2tr:0 --alkanes-change p2tr:0 --fee-rate $FEE_RATE)"
    if [ "$PREFLIGHT_ONLY" = "1" ]; then
        log_ok "PREFLIGHT_ONLY=1 — stopping here, nothing broadcast"
        exit 0
    fi

    # 1. The game. Its init never touches the operator contract — the key id
    #    is only compared against incoming alkanes later — so it can go first.
    local fresh=0
    if [ -z "$GAME_TXID" ]; then
        slot_occupied 4 "$GAME_TX" && die "4:$GAME_TX got occupied meanwhile — pick another GAME_TX"
        confirm "the GAME deploy (4:$GAME_TX, bet token $BET_TOKEN — immutable)"
        broadcast "$game_cellpack" "$GAME_WASM" "dice-game (4:$GAME_TX)"
        GAME_TXID="$LAST_TXID"
        write_state
        fresh=1
    fi
    wait_for_deploy "$GAME_TXID" 4 "$GAME_TX"
    verify_game "$fresh"
    [ "$VERIFY_FAILED" = "0" ] || die "game verification failed — inspect: alkanes trace $GAME_TXID:<vout> (operator NOT deployed)"

    # 2. The operator key, LAST: no deploy tx spends this wallet's UTXOs after
    #    the keys land. The game already trusts 4:$OPERATOR_TX, so the slot must
    #    still be free — re-checked right before the broadcast.
    if [ -z "$OPERATOR_TXID" ]; then
        if slot_occupied "$OPERATOR_BLOCK" "$OPERATOR_TX"; then
            die "4:$OPERATOR_TX was taken after the game went live — the game's fee key now belongs to whoever deployed it. Stop and inspect before anything else."
        fi
        confirm "the OPERATOR deploy (4:$OPERATOR_TX, mints the $KEY_SUPPLY keys)"
        broadcast "$op_cellpack" "$OPERATOR_WASM" "dice-operator (4:$OPERATOR_TX)"
        OPERATOR_TXID="$LAST_TXID"
        write_state
    fi
    wait_for_deploy "$OPERATOR_TXID" "$OPERATOR_BLOCK" "$OPERATOR_TX"
    verify_operator
    [ "$VERIFY_FAILED" = "0" ] || die "operator verification failed — inspect: alkanes trace $OPERATOR_TXID:<vout>"
    write_state

    log_phase "DEPLOYED"
    cat <<EOF

  operator auth keys  $KEY_SUPPLY × 4:$OPERATOR_TX   txid $OPERATOR_TXID
  game (template +    4:$GAME_TX   txid $GAME_TXID
    registry + $BET_TOKEN table)
  record              $STATE_FILE

  Next:
    1. Commit the record: git add deployments/ && git commit
    2. Split the $KEY_SUPPLY keys (4:$OPERATOR_TX) across separate alkanes-aware wallets:
       one to use, two spares. ANY one is full operator power on every table
       (WithdrawFees, SetExitFee <= 2%), and no more can ever be minted.
    3. The table is dormant until the first Deposit (op 2) seeds the bank.
    4. Point the frontend at 4:$GAME_TX.

EOF
}

main

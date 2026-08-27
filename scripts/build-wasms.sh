#!/bin/bash
# Build the CheekyD WASM contract (+ the test-only attacker fixture) and
# vendor them for the test harness. Pattern from arbuzino/scripts/build-wasms.sh.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR/.."
WASM_DIR="$PROJECT_DIR/target/wasm32-unknown-unknown/release"
TEST_WASM_DIR="$PROJECT_DIR/src/tests/wasm"

echo "Building CheekyD WASM contracts..."
cd "$PROJECT_DIR"

# cheekyd-game is the product; cheekyd-attacker is a TEST-ONLY fixture used by
# flash_attack_test (never deployed).
CC="/opt/homebrew/opt/llvm/bin/clang" cargo build --release --target wasm32-unknown-unknown -p cheekyd-game
CC="/opt/homebrew/opt/llvm/bin/clang" cargo build --release --target wasm32-unknown-unknown -p cheekyd-attacker

echo ""
echo "Built WASMs:"
ls -la "$WASM_DIR"/*.wasm 2>/dev/null || echo "No WASMs found"

mkdir -p "$TEST_WASM_DIR"
cp "$WASM_DIR/cheekyd_game.wasm" "$TEST_WASM_DIR/"
cp "$WASM_DIR/cheekyd_attacker.wasm" "$TEST_WASM_DIR/"
echo "Copied cheekyd_game.wasm + cheekyd_attacker.wasm to $TEST_WASM_DIR"
echo "(frost_token.wasm / alkanes_std_proxy.wasm are vendored build artifacts)"
echo "Done."

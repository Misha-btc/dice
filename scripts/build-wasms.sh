#!/bin/bash
# Build the Cheeky Bricky WASM contract (+ the test-only attacker fixture) and
# vendor them for the test harness. Pattern from arbuzino/scripts/build-wasms.sh.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR/.."
WASM_DIR="$PROJECT_DIR/target/wasm32-unknown-unknown/release"
TEST_WASM_DIR="$PROJECT_DIR/src/tests/wasm"

echo "Building Cheeky Bricky WASM contracts..."
cd "$PROJECT_DIR"

# Reproducible bytes: panic locations embed source paths, which would bake
# this machine's $HOME into the wasm — dependency sources under CARGO_HOME, and
# std sources under the toolchain's local rust-src (mapped back to the
# canonical /rustc/<commit> that a toolchain without rust-src would emit).
# Remap them so anyone can rebuild the deployed bytecode from a commit
# (toolchain: rust-toolchain.toml).
RUST_SRC="$(rustc --print sysroot)/lib/rustlib/src/rust"
RUSTC_COMMIT="$(rustc -vV | sed -n 's/^commit-hash: //p')"
export RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$(pwd -P)=/build --remap-path-prefix=$RUST_SRC=/rustc/$RUSTC_COMMIT"

# cheeky-bricky-game is the product; cheeky-bricky-attacker is a TEST-ONLY fixture used by
# flash_attack_test (never deployed).
CC="/opt/homebrew/opt/llvm/bin/clang" cargo build --release --target wasm32-unknown-unknown -p cheeky-bricky-game
CC="/opt/homebrew/opt/llvm/bin/clang" cargo build --release --target wasm32-unknown-unknown -p cheeky-bricky-attacker
CC="/opt/homebrew/opt/llvm/bin/clang" cargo build --release --target wasm32-unknown-unknown -p cheeky-bricky-operator

echo ""
echo "Built WASMs:"
ls -la "$WASM_DIR"/*.wasm 2>/dev/null || echo "No WASMs found"

mkdir -p "$TEST_WASM_DIR"
cp "$WASM_DIR/cheeky_bricky_game.wasm" "$TEST_WASM_DIR/"
cp "$WASM_DIR/cheeky_bricky_attacker.wasm" "$TEST_WASM_DIR/"
cp "$WASM_DIR/cheeky_bricky_operator.wasm" "$TEST_WASM_DIR/"
echo "Copied cheeky_bricky_game.wasm + attacker + operator to $TEST_WASM_DIR"
echo "(frost_token.wasm / alkanes_std_proxy.wasm are vendored build artifacts)"
echo "Done."

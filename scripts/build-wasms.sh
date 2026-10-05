#!/bin/bash
# Build the Dice WASM contracts (the game, the operator key, and the test-only
# attacker fixture) and copy them where the test harness loads them.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR/.."
WASM_DIR="$PROJECT_DIR/target/wasm32-unknown-unknown/release"
TEST_WASM_DIR="$PROJECT_DIR/src/tests/wasm"

echo "Building Dice WASM contracts..."
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

# secp256k1-sys compiles C code with $CC, and the output bytes depend on the
# clang version: the deployed contracts were built with Homebrew LLVM clang
# 20.1.4 (deployments/mainnet.env). Override with CC=/path/to/clang; any other
# clang builds working contracts, but not byte-identical to the deployed ones.
if [ -z "${CC:-}" ]; then
    if [ -x /opt/homebrew/opt/llvm/bin/clang ]; then
        CC=/opt/homebrew/opt/llvm/bin/clang
    else
        CC=clang
    fi
fi
export CC
echo "C compiler: $("$CC" --version | head -1)"

# dice-game is the product; dice-attacker is a TEST-ONLY fixture used by
# flash_attack_test (never deployed).
cargo build --release --target wasm32-unknown-unknown -p dice-game
cargo build --release --target wasm32-unknown-unknown -p dice-attacker
cargo build --release --target wasm32-unknown-unknown -p dice-operator

echo ""
echo "Built WASMs:"
ls -la "$WASM_DIR"/*.wasm 2>/dev/null || echo "No WASMs found"

mkdir -p "$TEST_WASM_DIR"
cp "$WASM_DIR/dice_game.wasm" "$TEST_WASM_DIR/"
cp "$WASM_DIR/dice_attacker.wasm" "$TEST_WASM_DIR/"
cp "$WASM_DIR/dice_operator.wasm" "$TEST_WASM_DIR/"
echo "Copied dice_game.wasm + attacker + operator to $TEST_WASM_DIR"
echo "Done."

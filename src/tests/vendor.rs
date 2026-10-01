//! WASM vendoring for tests. `scripts/build-wasms.sh` copies the built
//! release binary into `./wasm/` for include_bytes! to pick up.

pub fn get_dice_game_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/dice_game.wasm")
}

/// dice-attacker (TEST-ONLY) — a minimal wrapper alkane that forwards a
/// Roll to the game from its own contract context, to prove the flash-attack
/// guard rejects contract-wrapped rolls.
pub fn get_attacker_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/dice_attacker.wasm")
}

/// dice-operator — the operator auth key required by WithdrawFees.
pub fn get_operator_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/dice_operator.wasm")
}

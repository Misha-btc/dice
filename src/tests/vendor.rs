//! WASM vendoring for tests. `scripts/build-wasms.sh` copies the built
//! release binary into `./wasm/` for include_bytes! to pick up.

pub fn get_cheeky_bricky_game_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/cheeky_bricky_game.wasm")
}

/// cheeky-bricky-attacker (TEST-ONLY) — a minimal wrapper alkane that forwards a
/// Roll to the game from its own contract context, to prove the flash-attack
/// guard rejects contract-wrapped rolls.
pub fn get_attacker_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/cheeky_bricky_attacker.wasm")
}

/// cheeky-bricky-operator — the operator auth key required by WithdrawFees.
pub fn get_operator_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/cheeky_bricky_operator.wasm")
}

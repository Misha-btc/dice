//! WASM vendoring for tests. `scripts/build-wasms.sh` copies the built
//! release binary into `./wasm/` for include_bytes! to pick up.

pub fn get_cheekyd_game_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/cheekyd_game.wasm")
}

/// cheekyd-attacker (TEST-ONLY) — a minimal wrapper alkane that forwards a
/// Roll to the game from its own contract context, to prove the flash-attack
/// guard rejects contract-wrapped rolls.
pub fn get_attacker_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/cheekyd_attacker.wasm")
}

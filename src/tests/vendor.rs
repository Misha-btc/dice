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

/// cheeky-bricky-treasury — the operator fee collector the game sweeps into.
pub fn get_treasury_wasm_bytes() -> &'static [u8] {
    include_bytes!("./wasm/cheeky_bricky_treasury.wasm")
}

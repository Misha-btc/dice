//! Foundation helpers: deploy the Cheeky Bricky game and provide the bet token.
//! The harness free-mints DIESEL (2:0), so tests init the game with DIESEL
//! as the bet token; mainnet passes its own token id (2:490) at deploy —
//! the id is an immutable init parameter precisely so the harness can test
//! with a mintable currency.

use crate::tests::helpers::{
    create_deployment_block, get_sheet_by_outpoint, index_block,
    init_with_multiple_cellpacks_with_tx, AlkaneId, Cellpack, OutPoint, Result, DIESEL_ID,
};
use crate::tests::vendor::get_cheeky_bricky_game_wasm_bytes;
use crate::test_log;

/// Tx slot for the game singleton. Result lives at {4, GAME_TX}.
pub const GAME_TX: u128 = 0xCD;

/// Harness min_bet: comfortably above the runtime's 1000 floor, far below the
/// free-mint DIESEL amounts the tests route around.
pub const MIN_BET: u128 = 10_000;

pub fn game_id() -> AlkaneId {
    AlkaneId { block: 4, tx: GAME_TX }
}

/// Mint DIESEL via opcode 77 (free mint). Returns (amount, outpoint).
/// NOTE: the mint tx has a null input → identical txid every call, so two
/// mints ALIAS the same outpoint. Mint at most once per test.
pub fn mint_diesel(block_height: u32) -> Result<(u128, OutPoint)> {
    let cellpack = Cellpack {
        target: DIESEL_ID,
        inputs: vec![77],
    };
    let mint_block = init_with_multiple_cellpacks_with_tx(vec![vec![]], vec![cellpack]);
    index_block(&mint_block, block_height)?;

    let outpoint = OutPoint {
        txid: mint_block.txdata[1].compute_txid(),
        vout: 0,
    };
    let sheet = get_sheet_by_outpoint(&outpoint)?;
    let amount = sheet.get_cached(&DIESEL_ID.into());
    test_log!("[FOUNDATION] minted {} DIESEL at {:?}", amount, outpoint);
    Ok((amount, outpoint))
}

#[derive(Debug, Clone)]
pub struct CheekyBrickyDeployment {
    pub game_id: AlkaneId,
    pub last_height: u32,
}

/// Deploy + init the game in one cellpack (gotcha: deploy via block 3
/// IMMEDIATELY runs opcode 0, so the cellpack must carry the real init
/// args). Bet token = DIESEL (2:0), the only currency the harness can mint.
pub fn deploy_cheeky_bricky(min_bet: u128) -> Result<CheekyBrickyDeployment> {
    let mut height = 880_000u32;

    let game_deploy_block = create_deployment_block(
        height,
        get_cheeky_bricky_game_wasm_bytes(),
        Cellpack {
            target: AlkaneId { block: 3, tx: GAME_TX },
            // name/symbol 0 = the "Cheeky Bricky"/"CBRK" defaults
            inputs: vec![0, min_bet, DIESEL_ID.block, DIESEL_ID.tx, 0, 0],
        },
    );
    index_block(&game_deploy_block, height)?;
    height += 1;

    test_log!("[FOUNDATION] cheeky-bricky deployed: {:?}", game_id());
    Ok(CheekyBrickyDeployment {
        game_id: game_id(),
        last_height: height,
    })
}

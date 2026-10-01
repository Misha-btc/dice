//! Foundation helpers: deploy the Dice game and provide the bet token.
//! The harness free-mints DIESEL (2:0), so tests init the game with DIESEL
//! as the bet token; mainnet passes its own token id (2:490) at deploy —
//! the id is an immutable init parameter precisely so the harness can test
//! with a mintable currency.

use crate::tests::helpers::{
    create_deployment_block, get_sheet_by_outpoint, index_block,
    init_with_multiple_cellpacks_with_tx, AlkaneId, Cellpack, OutPoint, Result, DIESEL_ID,
};
use crate::tests::vendor::{get_dice_game_wasm_bytes, get_operator_wasm_bytes};
use crate::test_log;

/// Tx slot for the game singleton. Result lives at {4, GAME_TX} — the same
/// 4:0xFACE (64206) the mainnet deploy uses.
pub const GAME_TX: u128 = 0xFACE;

/// Tx slot for the operator auth key — MUST match the runtime's hardcoded
/// OPERATOR_AUTH_ID (4:0xDEAD). WithdrawFees/SetExitFee require one unit of it.
pub const OPERATOR_TX: u128 = 0xDEAD;

/// Key units the operator's init mints (any single one authorizes).
pub const OPERATOR_KEYS: u128 = 3;

/// The contract's hardcoded minimum bet (re-exported for tests). Bets below
/// this revert.
pub const MIN_BET: u128 = 1_000;

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
pub struct DiceDeployment {
    pub game_id: AlkaneId,
    pub last_height: u32,
}

/// Deploy + init the game in one cellpack (gotcha: deploy via block 3
/// IMMEDIATELY runs opcode 0, so the cellpack must carry the real init
/// args). Bet token = DIESEL (2:0), the only currency the harness can mint.
pub fn deploy_dice() -> Result<DiceDeployment> {
    let mut height = 880_000u32;

    let game_deploy_block = create_deployment_block(
        height,
        get_dice_game_wasm_bytes(),
        Cellpack {
            target: AlkaneId { block: 3, tx: GAME_TX },
            // name/symbol 0 = the "Dice"/"DICE" defaults
            inputs: vec![0, DIESEL_ID.block, DIESEL_ID.tx, 0, 0],
        },
    );
    index_block(&game_deploy_block, height)?;
    height += 1;

    test_log!("[FOUNDATION] dice deployed: {:?}", game_id());
    Ok(DiceDeployment {
        game_id: game_id(),
        last_height: height,
    })
}

/// Deploy the operator auth key at 4:0xDEAD (= the runtime's OPERATOR_AUTH_ID).
/// Init (opcode 0) mints `OPERATOR_KEYS` key units (units of itself) to the
/// deploy tx's vout 0. Returns (operator auth id, key outpoint, next height).
pub fn deploy_operator(height: u32) -> Result<(AlkaneId, OutPoint, u32)> {
    let block = create_deployment_block(
        height,
        get_operator_wasm_bytes(),
        Cellpack { target: AlkaneId { block: 3, tx: OPERATOR_TX }, inputs: vec![0] },
    );
    index_block(&block, height)?;
    let operator = AlkaneId { block: 4, tx: OPERATOR_TX };
    let key = OutPoint { txid: block.txdata[1].compute_txid(), vout: 0 };
    Ok((operator, key, height + 1))
}

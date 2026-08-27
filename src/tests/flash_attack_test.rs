//! The flash-attack guard: `Roll` must be a DIRECT top-level transaction
//! (`context.caller == {0,0}`). A wrapper alkane could otherwise observe the
//! block's roll and atomic-revert losses, turning the −edge game +EV and
//! draining the bank.
//!
//! Proven with `cheekyd-attacker`, a test-only alkane whose one job is to
//! forward a Roll to the game from ITS OWN contract context:
//!   * SANITY — the identical Roll sent TOP-LEVEL (caller {0,0}) is accepted.
//!   * THE GUARD — the attacker's wrapped Roll REVERTS: no bet recorded, the
//!     bank untouched, the DIESEL refunded. The only difference is the
//!     caller, so the revert is the guard firing.

use crate::test_log;
use crate::tests::foundation::{deploy_cheekyd, game_id, mint_diesel, MIN_BET};
use crate::tests::helpers::{
    clear_test_environment, create_deployment_block, create_op_block_with_token_input,
    get_sheet_by_outpoint, index_block, AlkaneId, Block, Cellpack, OutPoint, DIESEL_ID,
};
use crate::tests::vendor::get_attacker_wasm_bytes;
use anyhow::Result;

const OP_ROLL: u128 = 1;
const OP_DEPOSIT: u128 = 2;
const OP_ATTACK_ROLL: u128 = 1;
const VIEW_BANK: u128 = 101;
const VIEW_STATS: u128 = 102;
const ATTACKER_TX: u128 = 0x600;
const FUEL: u64 = 100_000_000;

fn u128s(id: &AlkaneId, op: u128) -> Vec<u128> {
    alkanes::view::call_view(id, &vec![op], FUEL)
        .unwrap_or_default()
        .chunks_exact(16)
        .map(|c| u128::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn op_outpoint(block: &Block) -> OutPoint {
    OutPoint { txid: block.txdata[1].compute_txid(), vout: 0 }
}

fn diesel_at(o: &OutPoint) -> u128 {
    get_sheet_by_outpoint(o).unwrap().get_cached(&DIESEL_ID.into())
}

#[test]
fn test_roll_rejects_contract_caller() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheekyd(MIN_BET)?;
    let mut h = deploy.last_height;
    let g = game_id();

    // Seed a bank so a roll would have something to pay from / clamp against.
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;
    let bank = 250_000_000u128;
    let dep = create_op_block_with_token_input(
        h,
        Cellpack { target: g.clone(), inputs: vec![OP_DEPOSIT, bank] },
        diesel_op,
        DIESEL_ID,
        minted,
    );
    index_block(&dep, h)?;
    let change_op = op_outpoint(&dep); // leftover DIESEL (minted − bank)
    h += 1;

    // ── SANITY: the same Roll works TOP-LEVEL (caller {0,0}) ─────────────
    let bet = 1_000_000u128;
    let direct = create_op_block_with_token_input(
        h,
        Cellpack { target: g.clone(), inputs: vec![OP_ROLL, 32_768, bet, 0] },
        change_op,
        DIESEL_ID,
        bet,
    );
    index_block(&direct, h)?;
    let after_direct = op_outpoint(&direct);
    assert_eq!(u128s(&g, VIEW_STATS)[0], 1, "sanity: top-level Roll accepted");
    h += 1;

    // Deploy the attacker (opcode 0 = no-op init).
    let atk_block = create_deployment_block(
        h,
        get_attacker_wasm_bytes(),
        Cellpack { target: AlkaneId { block: 3, tx: ATTACKER_TX }, inputs: vec![0] },
    );
    index_block(&atk_block, h)?;
    let attacker = AlkaneId { block: 4, tx: ATTACKER_TX };
    h += 1;

    // ── THE GUARD: attacker → game.Roll reverts ─────────────────────────
    let bets_before = u128s(&g, VIEW_STATS)[0];
    let bank_before = u128s(&g, VIEW_BANK)[0];
    let diesel_in = diesel_at(&after_direct); // all of it flows through the attack tx
    // Route the leftover DIESEL into the attacker, which forwards it to Roll.
    let attack = create_op_block_with_token_input(
        h,
        Cellpack {
            target: attacker.clone(),
            inputs: vec![OP_ATTACK_ROLL, g.block, g.tx, 32_768, bet, 0],
        },
        after_direct, // the DIESEL the top-level roll handed back as change
        DIESEL_ID,
        bet,
    );
    index_block(&attack, h)?;
    let attack_out = op_outpoint(&attack);

    assert_eq!(
        u128s(&g, VIEW_STATS)[0],
        bets_before,
        "guard: a contract-wrapped Roll records no bet"
    );
    assert_eq!(
        u128s(&g, VIEW_BANK)[0],
        bank_before,
        "guard: the bank is untouched by the rejected roll"
    );
    // Whole tx reverted → every DIESEL that entered comes back to vout 0
    // (the game captured nothing).
    assert_eq!(diesel_at(&attack_out), diesel_in, "guard: all DIESEL refunded on revert");
    test_log!("[FLASH] guard OK: contract-wrapped Roll reverted, nothing moved");
    Ok(())
}

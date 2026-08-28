//! LP-exit fee → operator, end to end.
//!
//! Every LP withdrawal keeps 2% (`EXIT_BPS`) of the gross payout in the table's
//! own `/fee_accum` (still the bet token); the LP receives 98%. `WithdrawFees`
//! (opcode 6) pays that accrued fee to whoever presents the hardcoded operator
//! auth key (4:0x7EA5) — one key drains every table the operator runs — and
//! zeroes the accumulator. No shared treasury, no cross-token registry: each
//! table holds its own single-token fee. This proves fee accrual, the auth
//! gate, and that the fee is observable via ViewBank's `fee_accum` field.

use crate::test_log;
use crate::tests::foundation::{
    deploy_cheeky_bricky, deploy_operator, game_id, mint_diesel,
};
use crate::tests::helpers::{
    clear_test_environment, create_op_block_with_token_input, create_operation_block,
    get_sheet_by_outpoint, index_block, AlkaneId, Cellpack, OutPoint, DIESEL_ID,
};
use anyhow::Result;
use cheeky_bricky_math::{shares_for_deposit, withdraw_payout};

const OP_DEPOSIT: u128 = 2;
const OP_WITHDRAW: u128 = 3;
const OP_WITHDRAW_FEES: u128 = 6;
const VIEW_BANK: u128 = 101;
const FUEL: u64 = 100_000_000;

fn op_outpoint(block: &bitcoin::Block) -> OutPoint {
    OutPoint { txid: block.txdata[1].compute_txid(), vout: 0 }
}

fn bal(outpoint: &OutPoint, id: AlkaneId) -> u128 {
    get_sheet_by_outpoint(outpoint).unwrap().get_cached(&id.into())
}

fn view(op: u128) -> Vec<u128> {
    alkanes::view::call_view(&game_id(), &vec![op], FUEL)
        .unwrap_or_default()
        .chunks_exact(16)
        .map(|c| u128::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Fresh env: deploy game + operator auth, deposit, withdraw ALL shares
/// (accruing a 2% fee). Returns `(fee, operator id, key outpoint, next_height)`.
fn accrue_fee() -> Result<(u128, AlkaneId, OutPoint, u32)> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;

    let (operator, key, next_h) = deploy_operator(h)?;
    h = next_h;

    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    let amount = 200_000_000u128;
    let total_shares = shares_for_deposit(amount, 0, 0);
    let dep = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_DEPOSIT, amount] },
        diesel_op,
        DIESEL_ID,
        minted,
    );
    index_block(&dep, h)?;
    let dep_out = op_outpoint(&dep);
    h += 1;

    let gross = withdraw_payout(total_shares, amount, total_shares);
    let fee = gross * 200 / 10_000;
    let lp_net = gross - fee;
    assert!(fee > 0, "fee must be non-zero for a meaningful test");

    let wd = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_WITHDRAW, total_shares, 0] },
        dep_out,
        game_id(),
        total_shares,
    );
    index_block(&wd, h)?;
    let ride = minted - amount;
    assert_eq!(bal(&op_outpoint(&wd), DIESEL_ID), ride + lp_net, "LP receives 98% net + change");
    h += 1;

    // ViewBank exposes the accrued fee in field 3.
    let vb = view(VIEW_BANK);
    assert_eq!(vb.get(2).copied(), Some(fee), "ViewBank reports fee_accum == fee");

    Ok((fee, operator, key, h))
}

#[test]
fn test_operator_withdraws_fee_with_auth_key() -> Result<()> {
    let (fee, operator, key, mut h) = accrue_fee()?;

    // Non-operator attempt (no key attached) must revert and move nothing.
    let steal = create_operation_block(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_WITHDRAW_FEES] },
        None,
    );
    index_block(&steal, h)?;
    h += 1;
    assert_eq!(view(VIEW_BANK).get(2).copied(), Some(fee), "keyless attempt left the fee");

    // Operator withdraws the fee by presenting the auth key; key returned.
    let wf = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_WITHDRAW_FEES] },
        key,
        operator,
        1,
    );
    index_block(&wf, h)?;
    let out = op_outpoint(&wf);
    assert_eq!(bal(&out, DIESEL_ID), fee, "operator receives exactly the accrued fee");
    assert_eq!(bal(&out, operator), 1, "auth key returned (spent-and-returned)");
    assert_eq!(view(VIEW_BANK).get(2).copied(), Some(0), "fee_accum zeroed after withdrawal");
    test_log!("[EXIT] operator withdrew fee {} with auth key", fee);
    Ok(())
}

#[test]
fn test_withdraw_fees_reverts_when_empty() -> Result<()> {
    // Fresh table, no LP activity → no fee. Even with the key, WithdrawFees
    // reverts (nothing to pay) and the key is refunded.
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let (operator, key, next_h) = deploy_operator(h)?;
    h = next_h;

    let wf = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_WITHDRAW_FEES] },
        key,
        operator,
        1,
    );
    index_block(&wf, h)?;
    let out = op_outpoint(&wf);
    assert_eq!(bal(&out, operator), 1, "revert refunds the auth key");
    assert_eq!(bal(&out, DIESEL_ID), 0, "no payout on empty fee");
    Ok(())
}

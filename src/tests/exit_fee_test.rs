//! LP-exit fee → operator treasury, end to end.
//!
//! A withdrawal keeps 2% (`EXIT_BPS`) of the gross payout in the game's
//! `/fee_accum` (still the bank token); the LP receives 98%. `SweepFees`
//! (opcode 6, permissionless, hardcoded destination) moves the accrued fee
//! into the treasury (4:0x7EA5). Only the treasury's owner — whoever holds one
//! self-unit key minted at deploy — can withdraw it. This proves the whole
//! wiring: fee accrual, the CEI sweep to the fixed sink, and owner-gated payout.

use crate::test_log;
use crate::tests::foundation::{
    deploy_cheeky_bricky, deploy_treasury, game_id, mint_diesel,
};
use crate::tests::helpers::{
    clear_test_environment, create_op_block_with_token_input, create_operation_block,
    get_sheet_by_outpoint, index_block, AlkaneId, Cellpack, OutPoint, DIESEL_ID,
};
use anyhow::Result;
use cheeky_bricky_math::{shares_for_deposit, withdraw_payout};

const OP_DEPOSIT: u128 = 2;
const OP_WITHDRAW: u128 = 3;
const OP_SWEEP_FEES: u128 = 6;
const TREASURY_WITHDRAW: u128 = 2;

fn op_outpoint(block: &bitcoin::Block) -> OutPoint {
    OutPoint { txid: block.txdata[1].compute_txid(), vout: 0 }
}

fn bal(outpoint: &OutPoint, id: AlkaneId) -> u128 {
    get_sheet_by_outpoint(outpoint).unwrap().get_cached(&id.into())
}

#[test]
fn test_exit_fee_accrues_sweeps_to_treasury_owner_withdraws() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;

    // Deploy the operator treasury at the hardcoded TREASURY_ID (4:0x7EA5).
    let (treasury, owner_key, next_h) = deploy_treasury(h)?;
    h = next_h;

    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    // Deposit → mint shares. Overpay (minted − amount) rides back as change.
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

    // Withdraw ALL shares. LP gets 98%; the 2% fee stays in the game's
    // /fee_accum (still bank-token DIESEL held by the contract).
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
    let wd_out = op_outpoint(&wd);
    h += 1;

    // The deposit change (minted − amount) also rode along to the withdraw out.
    let ride = minted - amount;
    assert_eq!(bal(&wd_out, DIESEL_ID), ride + lp_net, "LP receives 98% net + change");
    test_log!("[EXIT] gross {}, fee {}, lp_net {}", gross, fee, lp_net);

    // SweepFees: permissionless, no inputs. Moves the accrued fee into the
    // hardcoded treasury. (Zeroes /fee_accum before the external call — CEI.)
    let sweep = create_operation_block(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_SWEEP_FEES] },
        None,
    );
    index_block(&sweep, h)?;
    h += 1;

    // A second sweep must revert — nothing left to sweep. (Indexing a block
    // whose only tx reverts is a no-op; the treasury balance is unchanged,
    // which we confirm below by withdrawing EXACTLY `fee`.)
    let sweep2 = create_operation_block(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_SWEEP_FEES] },
        None,
    );
    index_block(&sweep2, h)?;
    h += 1;

    // Non-owner withdraw attempt (no key attached) must revert and move nothing.
    let steal = create_operation_block(
        h,
        Cellpack {
            target: treasury,
            inputs: vec![TREASURY_WITHDRAW, DIESEL_ID.block, DIESEL_ID.tx, fee],
        },
        None,
    );
    index_block(&steal, h)?;
    h += 1;

    // Owner withdraw: attach the 1-unit key, pull EXACTLY the swept fee. The
    // key is returned alongside the DIESEL.
    let owner_wd = create_op_block_with_token_input(
        h,
        Cellpack {
            target: treasury,
            inputs: vec![TREASURY_WITHDRAW, DIESEL_ID.block, DIESEL_ID.tx, fee],
        },
        owner_key,
        treasury,
        1,
    );
    index_block(&owner_wd, h)?;
    let owner_out = op_outpoint(&owner_wd);

    assert_eq!(bal(&owner_out, DIESEL_ID), fee, "owner extracts exactly the swept fee");
    assert_eq!(bal(&owner_out, treasury), 1, "owner key returned");
    test_log!("[EXIT] treasury owner withdrew fee {} + key back", fee);
    Ok(())
}

//! LP-side integration: deposit mints shares at the virtual-offset price,
//! withdraw burns them for a pro-rata slice of the bank, donate grows the
//! bank without minting. Economics are proven in `cheeky-bricky-math`; these prove
//! the on-chain wiring (mint/burn of self-units, change returns, refunds).

use crate::test_log;
use crate::tests::foundation::{deploy_cheeky_bricky, game_id, mint_diesel, MIN_BET};
use crate::tests::helpers::{
    clear_test_environment, create_op_block_with_token_input, get_sheet_by_outpoint, index_block,
    AlkaneId, Cellpack, OutPoint, DIESEL_ID,
};
use anyhow::Result;
use cheeky_bricky_math::{shares_for_deposit, withdraw_payout};

const OP_DEPOSIT: u128 = 2;
const OP_WITHDRAW: u128 = 3;
const OP_DONATE: u128 = 5;
const VIEW_BANK: u128 = 101;
const FUEL: u64 = 100_000_000;

fn call_view(op: u128) -> Vec<u128> {
    let bytes = alkanes::view::call_view(&game_id(), &vec![op], FUEL).unwrap_or_default();
    bytes
        .chunks_exact(16)
        .map(|c| u128::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn op_outpoint(block: &bitcoin::Block) -> OutPoint {
    OutPoint {
        txid: block.txdata[1].compute_txid(),
        vout: 0,
    }
}

fn diesel_at(outpoint: &OutPoint) -> u128 {
    get_sheet_by_outpoint(outpoint)
        .unwrap()
        .get_cached(&DIESEL_ID.into())
}

fn shares_at(outpoint: &OutPoint) -> u128 {
    get_sheet_by_outpoint(outpoint)
        .unwrap()
        .get_cached(&game_id().into())
}

/// Route `route` DIESEL from `input` into an opcode call with `inputs`.
fn run_op(
    height: u32,
    inputs: Vec<u128>,
    input: OutPoint,
    token: AlkaneId,
    route: u128,
) -> Result<OutPoint> {
    let block = create_op_block_with_token_input(
        height,
        Cellpack { target: game_id(), inputs },
        input,
        token,
        route,
    );
    index_block(&block, height)?;
    Ok(op_outpoint(&block))
}

#[test]
fn test_deposit_mints_shares_and_returns_change() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    let amount = 200_000_000u128;
    let expected_shares = shares_for_deposit(amount, 0, 0);
    let out = run_op(h, vec![OP_DEPOSIT, amount], diesel_op, DIESEL_ID, minted)?;

    assert_eq!(shares_at(&out), expected_shares, "minted shares");
    assert_eq!(diesel_at(&out), minted - amount, "overpay returned as change");

    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], amount, "bank");
    assert_eq!(v[1], expected_shares, "total_shares");
    test_log!("[LP] deposit ok: {} shares, {} change", expected_shares, minted - amount);
    Ok(())
}

#[test]
fn test_withdraw_burns_shares_and_pays_pro_rata() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    let amount = 200_000_000u128;
    let total_shares = shares_for_deposit(amount, 0, 0);
    let dep_out = run_op(h, vec![OP_DEPOSIT, amount], diesel_op, DIESEL_ID, minted)?;
    h += 1;

    // Burn half the shares. The deposit outpoint also holds the DIESEL change,
    // which is NOT edicted into the call — it rides along to the new vout 0.
    let burn = total_shares / 2;
    let gross_payout = withdraw_payout(burn, amount, total_shares);
    // 2% LP-exit fee is carved off the gross payout: the LP receives 98%, the
    // fee accrues in /fee_accum (still in the contract's bank-token balance).
    let fee = gross_payout * 200 / 10_000;
    let lp_payout = gross_payout - fee;
    let ride_along = minted - amount;
    let out = run_op(h, vec![OP_WITHDRAW, burn, 0], dep_out, game_id(), burn)?;

    assert_eq!(
        diesel_at(&out),
        ride_along + lp_payout,
        "net (98%) payout + riding change"
    );
    assert_eq!(shares_at(&out), total_shares - burn, "unburned shares ride along");

    let v = call_view(VIEW_BANK);
    // Bank drops by the FULL gross payout; the fee just moved from bank into the
    // separate /fee_accum, still held by the contract.
    assert_eq!(v[0], amount - gross_payout, "bank drained by the gross payout");
    assert_eq!(v[1], total_shares - burn, "supply dropped by the burn");
    assert_eq!(v[2], fee, "fee_accum holds the 2% exit carve-out");
    test_log!("[LP] withdraw ok: burned {}, gross {}, fee {}, lp {}", burn, gross_payout, fee, lp_payout);
    Ok(())
}

#[test]
fn test_withdraw_slippage_floor_reverts() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    let amount = 200_000_000u128;
    let total_shares = shares_for_deposit(amount, 0, 0);
    let dep_out = run_op(h, vec![OP_DEPOSIT, amount], diesel_op, DIESEL_ID, minted)?;
    h += 1;

    // Demand more than the pro-rata payout → revert, shares refunded.
    let burn = total_shares / 2;
    let too_much = withdraw_payout(burn, amount, total_shares) + 1;
    let out = run_op(h, vec![OP_WITHDRAW, burn, too_much], dep_out, game_id(), burn)?;

    assert_eq!(shares_at(&out), total_shares, "revert refunds ALL shares (burned nothing)");
    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], amount, "bank untouched");
    assert_eq!(v[1], total_shares, "supply untouched");
    Ok(())
}

#[test]
fn test_donate_grows_bank_without_shares() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    let amount = 100_000_000u128;
    let out = run_op(h, vec![OP_DONATE, amount], diesel_op, DIESEL_ID, minted)?;

    assert_eq!(shares_at(&out), 0, "donation mints nothing");
    assert_eq!(diesel_at(&out), minted - amount, "overpay returned as change");
    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], amount, "bank grew by the donation");
    assert_eq!(v[1], 0, "no shares outstanding");
    Ok(())
}

#[test]
fn test_deposit_underpaid_reverts_and_refunds() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    // Ask to deposit more than we send → revert; the routed DIESEL refunds.
    let out = run_op(h, vec![OP_DEPOSIT, minted + 1], diesel_op, DIESEL_ID, minted)?;

    assert_eq!(diesel_at(&out), minted, "full refund on revert");
    assert_eq!(shares_at(&out), 0, "no shares minted");
    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], 0, "bank untouched");
    Ok(())
}

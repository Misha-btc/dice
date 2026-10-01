//! LP-exit fee → operator, end to end.
//!
//! Every LP withdrawal keeps the live exit fee (default 2% = `EXIT_BPS_MAX`,
//! operator-adjustable in `[0, 2%]` via `SetExitFee`) of the gross payout in the table's
//! own `/fee_accum` (still the bet token); the LP receives 98%. `WithdrawFees`
//! (opcode 6) pays that accrued fee to whoever presents the hardcoded operator
//! auth key (4:0xDEAD; init mints three interchangeable units and any one
//! authorizes) — one key drains every table the operator runs — and
//! zeroes the accumulator. No shared treasury, no cross-token registry: each
//! table holds its own single-token fee. This proves fee accrual, the auth
//! gate, and that the fee is observable via ViewBank's `fee_accum` field.

use crate::test_log;
use crate::tests::foundation::{
    deploy_dice, deploy_operator, game_id, mint_diesel, OPERATOR_KEYS,
};
use crate::tests::helpers::{
    clear_test_environment, create_op_block_with_token_input, create_operation_block,
    get_sheet_by_outpoint, index_block, AlkaneId, Cellpack, OutPoint, DIESEL_ID,
};
use anyhow::Result;
use dice_math::{shares_for_deposit, withdraw_payout};
use protorune_support::rune_transfer::RuneTransfer;

const OP_DEPOSIT: u128 = 2;
const OP_WITHDRAW: u128 = 3;
const OP_WITHDRAW_FEES: u128 = 6;
const OP_SET_EXIT_FEE: u128 = 7;
const VIEW_BANK: u128 = 101;
const VIEW_CONFIG: u128 = 103;
const EXIT_BPS_MAX: u128 = 200;
const FUEL: u64 = 100_000_000;

fn op_outpoint(block: &bitcoin::Block) -> OutPoint {
    OutPoint { txid: block.txdata[1].compute_txid(), vout: 0 }
}

fn bal(outpoint: &OutPoint, id: AlkaneId) -> u128 {
    get_sheet_by_outpoint(outpoint).unwrap().get_cached(&id.into())
}

/// Simulate a call on the game with exactly `runes` attached (nothing is
/// committed). Returns the alkanes handed back, or the revert.
fn simulate(inputs: Vec<u128>, runes: Vec<RuneTransfer>) -> Result<Vec<(AlkaneId, u128)>> {
    let mut parcel = alkanes::view::plain_parcel_from_cellpack(Cellpack { target: game_id(), inputs });
    parcel.runes = runes;
    parcel.height = 880_050;
    let (resp, _gas) = alkanes::view::simulate_parcel(&parcel, FUEL)?;
    Ok(resp.alkanes.0.into_iter().map(|t| (t.id, t.value)).collect())
}

fn rune(id: AlkaneId, value: u128) -> RuneTransfer {
    RuneTransfer { id: id.into(), value }
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
    let deploy = deploy_dice()?;
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
    assert_eq!(bal(&out, operator), OPERATOR_KEYS, "every key unit comes back (spent-and-returned)");
    assert_eq!(view(VIEW_BANK).get(2).copied(), Some(0), "fee_accum zeroed after withdrawal");
    test_log!("[EXIT] operator withdrew fee {} with auth key", fee);
    Ok(())
}

/// The operator init mints three interchangeable key units (spares against
/// loss), and ANY single unit authorizes: simulate the fee calls with exactly
/// one unit attached, then with none.
#[test]
fn test_three_keys_minted_any_single_one_authorizes() -> Result<()> {
    let (fee, operator, key, _h) = accrue_fee()?;
    assert_eq!(bal(&key, operator), OPERATOR_KEYS, "init mints three key units");

    let paid = simulate(vec![OP_WITHDRAW_FEES], vec![rune(operator.clone(), 1)])
        .expect("one key unit authorizes WithdrawFees");
    assert!(paid.contains(&(DIESEL_ID, fee)), "pays the whole accrued fee: {:?}", paid);
    assert!(paid.contains(&(operator.clone(), 1)), "hands the one key back: {:?}", paid);
    assert!(
        simulate(vec![OP_SET_EXIT_FEE, 100], vec![rune(operator.clone(), 1)]).is_ok(),
        "one key unit authorizes SetExitFee"
    );
    assert!(simulate(vec![OP_WITHDRAW_FEES], vec![]).is_err(), "no key, no fees");
    assert!(simulate(vec![OP_SET_EXIT_FEE, 100], vec![]).is_err(), "no key, no fee change");
    test_log!("[EXIT] {} key units minted; any single one authorizes", OPERATOR_KEYS);
    Ok(())
}

/// Deploy game + operator, mint DIESEL, and set the exit fee to `bps` using the
/// auth key. Returns `(diesel_op, minted, operator, key, next_height)`.
fn deploy_and_set_fee(bps: u128) -> Result<(OutPoint, u128, AlkaneId, OutPoint, u32)> {
    clear_test_environment();
    let deploy = deploy_dice()?;
    let mut h = deploy.last_height;
    let (operator, mut key, next_h) = deploy_operator(h)?;
    h = next_h;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    let sf = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_SET_EXIT_FEE, bps] },
        key,
        operator,
        1,
    );
    index_block(&sf, h)?;
    key = op_outpoint(&sf); // the key rode back here
    h += 1;
    assert_eq!(view(VIEW_BANK).get(3).copied(), Some(bps), "ViewBank field 4 == set fee");
    Ok((diesel_op, minted, operator, key, h))
}

/// Deposit `amount` then withdraw ALL shares; returns `(gross, lp_net_received)`.
fn deposit_then_full_withdraw(
    diesel_op: OutPoint,
    minted: u128,
    amount: u128,
    h: &mut u32,
) -> Result<(u128, u128)> {
    let total_shares = shares_for_deposit(amount, 0, 0);
    let dep = create_op_block_with_token_input(
        *h,
        Cellpack { target: game_id(), inputs: vec![OP_DEPOSIT, amount] },
        diesel_op,
        DIESEL_ID,
        minted,
    );
    index_block(&dep, *h)?;
    let dep_out = op_outpoint(&dep);
    *h += 1;

    let gross = withdraw_payout(total_shares, amount, total_shares);
    let wd = create_op_block_with_token_input(
        *h,
        Cellpack { target: game_id(), inputs: vec![OP_WITHDRAW, total_shares, 0] },
        dep_out,
        game_id(),
        total_shares,
    );
    index_block(&wd, *h)?;
    let ride = minted - amount;
    let got = bal(&op_outpoint(&wd), DIESEL_ID);
    *h += 1;
    Ok((gross, got - ride)) // subtract the deposit-change that rode along
}

#[test]
fn test_set_exit_fee_lowers_the_cut() -> Result<()> {
    // Operator sets the fee to 1% (100 bps). A withdrawal then nets 99%.
    let (diesel_op, minted, _op, _key, mut h) = deploy_and_set_fee(100)?;
    let amount = 200_000_000u128;
    let (gross, lp_net) = deposit_then_full_withdraw(diesel_op, minted, amount, &mut h)?;
    let fee = gross * 100 / 10_000;
    assert_eq!(lp_net, gross - fee, "LP nets 99% at a 1% fee");
    assert_eq!(view(VIEW_BANK).get(2).copied(), Some(fee), "fee_accum holds the 1% cut");
    test_log!("[EXIT] fee=100bps: gross {}, fee {}, lp {}", gross, fee, lp_net);
    Ok(())
}

#[test]
fn test_set_exit_fee_zero_means_free_exit() -> Result<()> {
    // Operator sets the fee to 0. A withdrawal nets 100%; nothing accrues.
    let (diesel_op, minted, _op, _key, mut h) = deploy_and_set_fee(0)?;
    let amount = 200_000_000u128;
    let (gross, lp_net) = deposit_then_full_withdraw(diesel_op, minted, amount, &mut h)?;
    assert_eq!(lp_net, gross, "LP nets 100% at a 0% fee");
    assert_eq!(view(VIEW_BANK).get(2).copied(), Some(0), "no fee accrues");
    Ok(())
}

#[test]
fn test_set_exit_fee_cannot_exceed_max_or_skip_auth() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_dice()?;
    let mut h = deploy.last_height;
    let (operator, key, next_h) = deploy_operator(h)?;
    h = next_h;

    // Default fee is the 2% max; ViewConfig field 7 reports the immutable ceiling.
    assert_eq!(view(VIEW_BANK).get(3).copied(), Some(EXIT_BPS_MAX), "default fee = max");
    assert_eq!(view(VIEW_CONFIG).get(6).copied(), Some(EXIT_BPS_MAX), "ViewConfig exposes the max");

    // Above the ceiling reverts (with the key) — fee unchanged.
    let over = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_SET_EXIT_FEE, EXIT_BPS_MAX + 1] },
        key,
        operator,
        1,
    );
    index_block(&over, h)?;
    let key2 = op_outpoint(&over); // key refunded on revert
    h += 1;
    assert_eq!(view(VIEW_BANK).get(3).copied(), Some(EXIT_BPS_MAX), "over-max rejected, fee unchanged");

    // Without the key reverts too — even a valid value.
    let noauth = create_operation_block(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_SET_EXIT_FEE, 50] },
        None,
    );
    index_block(&noauth, h)?;
    h += 1;
    assert_eq!(view(VIEW_BANK).get(3).copied(), Some(EXIT_BPS_MAX), "keyless set rejected");

    // A valid in-range set with the key sticks.
    let ok = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_SET_EXIT_FEE, 50] },
        key2,
        operator,
        1,
    );
    index_block(&ok, h)?;
    assert_eq!(view(VIEW_BANK).get(3).copied(), Some(50), "in-range set applied");
    Ok(())
}

#[test]
fn test_withdraw_fees_reverts_when_empty() -> Result<()> {
    // Fresh table, no LP activity → no fee. Even with the key, WithdrawFees
    // reverts (nothing to pay) and the key is refunded.
    clear_test_environment();
    let deploy = deploy_dice()?;
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
    assert_eq!(bal(&out, operator), OPERATOR_KEYS, "revert refunds every key unit");
    assert_eq!(bal(&out, DIESEL_ID), 0, "no payout on empty fee");
    Ok(())
}

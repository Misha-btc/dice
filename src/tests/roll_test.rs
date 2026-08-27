//! The game itself: rolls settle atomically from `sha256(blockhash ‖ txid)`.
//!
//! **Determinism trick:** the harness block header is fixed once the block is
//! created (pushing txdata does not recompute the merkle root), and the roll
//! tx's txid depends on its runestone payload — so varying the *routed* token
//! amount varies the txid, and with it the roll. `build_roll_forced` walks
//! route candidates until the PREDICTED outcome (computed with the very
//! `cheekyd_math::roll` the contract runs) is the one the test wants, then
//! indexes that block. Both branches are therefore covered deterministically,
//! and every test doubles as a proof that the contract's entropy matches the
//! spec formula bit-for-bit.

use crate::test_log;
use crate::tests::foundation::{deploy_cheekyd, game_id, mint_diesel, MIN_BET};
use crate::tests::helpers::{
    clear_test_environment, create_op_block_with_token_input, get_sheet_by_outpoint, index_block,
    Cellpack, OutPoint, DIESEL_ID,
};
use anyhow::Result;
use bitcoin::hashes::Hash;
use cheekyd_math::{
    is_win, max_bet, max_net_profit, payout_total, shares_for_deposit, t_max, EDGE_BPS,
};

const OP_ROLL: u128 = 1;
const OP_DEPOSIT: u128 = 2;
const VIEW_BANK: u128 = 101;
const VIEW_STATS: u128 = 102;
const VIEW_MAX_BET: u128 = 104;
const FUEL: u64 = 100_000_000;
const E: u128 = EDGE_BPS; // 100 — fixed forever

fn call_view(op: u128) -> Vec<u128> {
    let bytes = alkanes::view::call_view(&game_id(), &vec![op], FUEL).unwrap_or_default();
    bytes
        .chunks_exact(16)
        .map(|c| u128::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn call_view_args(op: u128, args: &[u128]) -> Vec<u128> {
    let mut inputs = vec![op];
    inputs.extend_from_slice(args);
    let bytes = alkanes::view::call_view(&game_id(), &inputs, FUEL).unwrap_or_default();
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

/// Build (unindexed) a Roll block and predict its roll with the spec formula.
fn build_roll(
    height: u32,
    input: OutPoint,
    route: u128,
    target: u128,
    amount: u128,
    min_amount: u128,
) -> (bitcoin::Block, u16) {
    let block = create_op_block_with_token_input(
        height,
        Cellpack {
            target: game_id(),
            inputs: vec![OP_ROLL, target, amount, min_amount],
        },
        input,
        DIESEL_ID,
        route,
    );
    let blockhash = block.header.block_hash().as_raw_hash().to_byte_array();
    let txid = block.txdata[1]
        .compute_txid()
        .as_raw_hash()
        .to_byte_array();
    let r = cheekyd_math::roll(&blockhash, &txid);
    (block, r)
}

/// Walk route candidates downward until the predicted outcome equals
/// `want_win`, then return the block + prediction + the route actually used.
fn build_roll_forced(
    height: u32,
    input: OutPoint,
    route_base: u128,
    target: u128,
    amount: u128,
    want_win: bool,
) -> (bitcoin::Block, u16, u128) {
    for k in 0..512u128 {
        let route = route_base - k;
        assert!(route >= amount, "forcing walked below the bet amount");
        let (block, r) = build_roll(height, input, route, target, amount, 0);
        if is_win(r, target) == want_win {
            return (block, r, route);
        }
    }
    panic!("could not force outcome in 512 tries");
}

/// Deploy, mint DIESEL, seed the bank via Deposit. Returns
/// (deposit_outpoint carrying the leftover DIESEL + all shares, leftover, next height).
fn setup_with_bank(bank: u128) -> Result<(OutPoint, u128, u32)> {
    clear_test_environment();
    let deploy = deploy_cheekyd(MIN_BET)?;
    let mut h = deploy.last_height;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;
    assert!(minted > bank, "harness mint too small for requested bank");

    let block = create_op_block_with_token_input(
        h,
        Cellpack {
            target: game_id(),
            inputs: vec![OP_DEPOSIT, bank],
        },
        diesel_op,
        DIESEL_ID,
        minted,
    );
    index_block(&block, h)?;
    h += 1;
    Ok((op_outpoint(&block), minted - bank, h))
}

const BANK: u128 = 250_000_000; // cap = 0.5% = 1_250_000 at the 1% edge

#[test]
fn test_win_pays_from_bank_atomically() -> Result<()> {
    let (bank_op, leftover, h) = setup_with_bank(BANK)?;

    let amount = 1_000_000u128;
    let target = 32_768u128; // ×1.98 at the 1% edge
    let (block, roll, _route) = build_roll_forced(h, bank_op, leftover, target, amount, true);
    index_block(&block, h)?;
    let out = op_outpoint(&block);

    let payout = payout_total(amount, target, E); // 1_980_000

    // The forcing walk may route less than `leftover` into the call, but the
    // un-edicted remainder rides to the same vout 0 — so the outpoint always
    // ends up with (leftover − amount) + payout regardless of the route used.
    assert_eq!(
        diesel_at(&out),
        (leftover - amount) + payout,
        "change + payout on the bet tx's own output"
    );
    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], BANK + amount - payout, "bank paid the win");
    let s = call_view(VIEW_STATS);
    assert_eq!((s[0], s[1], s[2]), (1, amount, payout));
    test_log!("[ROLL] forced WIN with roll {} < {}", roll, target);
    Ok(())
}

#[test]
fn test_lose_feeds_the_bank() -> Result<()> {
    let (bank_op, leftover, h) = setup_with_bank(BANK)?;

    let amount = 1_000_000u128;
    let target = 32_768u128;
    let (block, roll, _route) = build_roll_forced(h, bank_op, leftover, target, amount, false);
    index_block(&block, h)?;
    let out = op_outpoint(&block);

    // leftover − amount: change from the call + any un-routed remainder, both
    // landing on vout 0 (see the win test's note).
    assert_eq!(diesel_at(&out), leftover - amount, "only the change comes back");
    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], BANK + amount, "the whole stake joined the bank — LPs earn it all");
    let s = call_view(VIEW_STATS);
    assert_eq!((s[0], s[1], s[2]), (1, amount, 0));
    test_log!("[ROLL] forced LOSE with roll {} >= {}", roll, target);
    Ok(())
}

#[test]
fn test_oversized_bet_clamps_to_the_cap() -> Result<()> {
    let (bank_op, leftover, h) = setup_with_bank(BANK)?;

    // 10M at ×1.98 → net exposure ≈ 9.8M ≫ cap 1.25M → the bet is CLAMPED
    // down to max_bet (≈1.28M), not reverted; the excess comes back as
    // change. Force a lose so the accounting below is one-branch.
    let amount = 10_000_000u128;
    let target = 32_768u128;
    let clamped = max_bet(BANK, target, E);
    assert!(clamped < amount, "test premise: the bet must exceed the cap");

    let (block, _roll, _route) = build_roll_forced(h, bank_op, leftover, target, amount, false);
    index_block(&block, h)?;
    let out = op_outpoint(&block);

    assert_eq!(
        diesel_at(&out),
        leftover - clamped,
        "only the CLAMPED amount was taken; the rest returned"
    );
    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], BANK + clamped, "bank grew by the clamped stake");
    let s = call_view(VIEW_STATS);
    assert_eq!(s[1], clamped, "stats record the effective amount");
    Ok(())
}

#[test]
fn test_zero_amount_bets_everything_sent() -> Result<()> {
    // The mint-and-roll flow: `amount = 0` stakes the WHOLE incoming amount —
    // the tx that chains [free mint] → [Roll(target, 0)] can't know the
    // mint's size in advance, so the contract takes whatever arrived.
    //
    // Harness note: the whole outpoint balance reaches the call regardless of
    // the edict amount, so to model a small unknown "mint" we deposit almost
    // the entire free-mint into the bank and play with the 3M remainder.
    let big_bank = 311_500_000u128; // mint 312.5M − 1M "mint-sized" leftover
    let (bank_op, leftover, h) = setup_with_bank(big_bank)?;
    assert_eq!(leftover, 1_000_000, "test premise: small leftover to stake");
    let target = 32_768u128;
    assert!(leftover < max_bet(big_bank, target, E), "premise: no clamp");

    let (block, _roll, _route) = build_roll_forced(h, bank_op, leftover, target, 0, false);
    index_block(&block, h)?;
    let out = op_outpoint(&block);

    // The whole incoming became the stake — no change comes back on a lose.
    assert_eq!(diesel_at(&out), 0, "everything sent was staked");
    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], big_bank + leftover, "bank grew by the full incoming");
    let s = call_view(VIEW_STATS);
    assert_eq!(s[1], leftover, "stats record the full incoming as the bet");
    Ok(())
}

#[test]
fn test_min_amount_floor_reverts_a_clamp() -> Result<()> {
    let (bank_op, leftover, h) = setup_with_bank(BANK)?;

    // Same oversized bet, but the caller demands the full 10M (min_amount =
    // amount) → all-or-nothing → revert with a full refund.
    let amount = 10_000_000u128;
    let (block, _) = build_roll(h, bank_op, leftover, 32_768, amount, amount);
    index_block(&block, h)?;
    let out = op_outpoint(&block);

    assert_eq!(diesel_at(&out), leftover, "full refund on min_amount revert");
    let v = call_view(VIEW_BANK);
    assert_eq!(v[0], BANK, "bank untouched");
    assert_eq!(call_view(VIEW_STATS)[0], 0, "no bet recorded");
    Ok(())
}

#[test]
fn test_bad_target_and_dust_bet_revert() -> Result<()> {
    let (bank_op, leftover, mut h) = setup_with_bank(BANK)?;

    // Target below T_MIN.
    let (block, _) = build_roll(h, bank_op, leftover, 100, 1_000_000, 0);
    index_block(&block, h)?;
    let out = op_outpoint(&block);
    assert_eq!(diesel_at(&out), leftover, "bad target: full refund");
    h += 1;

    // Target above the edge-derived ceiling.
    let (block, _) = build_roll(h, out, leftover, t_max(E) + 1, 1_000_000, 0);
    index_block(&block, h)?;
    let out2 = op_outpoint(&block);
    assert_eq!(diesel_at(&out2), leftover, "target above t_max: full refund");
    h += 1;

    // Bet below min_bet.
    let (block, _) = build_roll(h, out2, leftover, 32_768, MIN_BET - 1, 0);
    index_block(&block, h)?;
    let out3 = op_outpoint(&block);
    assert_eq!(diesel_at(&out3), leftover, "dust bet: full refund");

    assert_eq!(call_view(VIEW_STATS)[0], 0, "nothing recorded");
    Ok(())
}

#[test]
fn test_view_max_bet_matches_math() -> Result<()> {
    let (_, _, _) = setup_with_bank(BANK)?;
    for target in [656u128, 6_554, 32_768, t_max(E)] {
        let v = call_view_args(VIEW_MAX_BET, &[target]);
        assert_eq!(v[0], max_bet(BANK, target, E), "max_bet at {}", target);
        assert_eq!(v[1], max_net_profit(BANK, E), "cap");
    }
    Ok(())
}

#[test]
fn test_shares_price_appreciates_after_losses() -> Result<()> {
    // An LP's withdrawable value grows when players lose: deposit, force a
    // lose, then check the same shares now redeem for more than the deposit.
    let (bank_op, leftover, h) = setup_with_bank(BANK)?;
    let total_shares = shares_for_deposit(BANK, 0, 0);

    let amount = 1_000_000u128;
    let (block, _, _) = build_roll_forced(h, bank_op, leftover, 32_768, amount, false);
    index_block(&block, h)?;

    let bank_now = call_view(VIEW_BANK)[0];
    let value_now = cheekyd_math::withdraw_payout(total_shares, bank_now, total_shares);
    assert!(
        value_now > BANK,
        "LP value didn't appreciate: {} ≤ {}",
        value_now,
        BANK
    );
    Ok(())
}

//! End-to-end scenarios the unit sims cover in the abstract but the wiring
//! did not: the per-block payout budget across two wins in one block, and a
//! full LP exit (bank → virtual dust) followed by a fresh deposit reviving
//! the table.

use crate::test_log;
use crate::tests::foundation::{deploy_cheeky_bricky, game_id, mint_diesel, MIN_BET};
use crate::tests::helpers::{
    clear_test_environment, create_op_block_with_token_input, get_sheet_by_outpoint, index_block,
    Block, Cellpack, OutPoint, DIESEL_ID,
};
use anyhow::Result;
use bitcoin::hashes::Hash;
use cheeky_bricky_math::{
    block_budget, is_win, max_amount_for_exposure, max_bet, max_net_profit, payout_total, shares_for_deposit,
    t_max, withdraw_payout, EDGE_BPS,
};

const OP_ROLL: u128 = 1;
const OP_DEPOSIT: u128 = 2;
const OP_WITHDRAW: u128 = 3;
const VIEW_BANK: u128 = 101;
const VIEW_STATS: u128 = 102;
const FUEL: u64 = 100_000_000;
const E: u128 = EDGE_BPS;

fn call_view(op: u128) -> Vec<u128> {
    alkanes::view::call_view(&game_id(), &vec![op], FUEL)
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

fn shares_at(o: &OutPoint) -> u128 {
    get_sheet_by_outpoint(o).unwrap().get_cached(&game_id().into())
}

/// Build (unindexed) a Roll block and predict its roll from the spec formula.
fn build_roll(height: u32, input: OutPoint, route: u128, target: u128, amount: u128) -> (Block, u16) {
    let block = create_op_block_with_token_input(
        height,
        Cellpack { target: game_id(), inputs: vec![OP_ROLL, target, amount, 0] },
        input,
        DIESEL_ID,
        route,
    );
    let bh = block.header.block_hash().as_raw_hash().to_byte_array();
    let txid = block.txdata[1].compute_txid().as_raw_hash().to_byte_array();
    (block.clone(), cheeky_bricky_math::roll(&bh, &txid))
}

/// Walk route candidates until the predicted outcome matches `want_win`.
fn forced_win(height: u32, input: OutPoint, route_base: u128, target: u128, amount: u128, want: bool) -> (Block, u128) {
    for k in 0..512u128 {
        let route = route_base - k;
        assert!(route >= amount, "walked below the bet");
        let (block, r) = build_roll(height, input, route, target, amount);
        if is_win(r, target) == want {
            return (block, route);
        }
    }
    panic!("could not force outcome");
}

fn setup_bank(bank: u128) -> Result<(OutPoint, u128, u32)> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;
    let block = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_DEPOSIT, bank] },
        diesel_op,
        DIESEL_ID,
        minted,
    );
    index_block(&block, h)?;
    h += 1;
    Ok((op_outpoint(&block), minted - bank, h))
}

/// Two forced wins in the SAME block: the second is bounded by the remaining
/// per-block budget (block=20% of bank, per-bet cap=1% full Kelly at the launch
/// edge → the block budget is 20 max-wins deep), so two ordinary wins fit
/// comfortably — this proves the budget accounting tracks realized wins
/// across bets in one height without bricking the second.
#[test]
fn test_two_wins_one_block_share_budget() -> Result<()> {
    let bank = 250_000_000u128;
    let (bank_op, leftover, h) = setup_bank(bank)?;
    let target = 32_768u128;
    let amount = 1_000_000u128;

    // First forced win.
    let (b1, route1) = forced_win(h, bank_op, leftover, target, amount, true);
    // Chain the second bet on the SAME height off b1's change output.
    let out1 = op_outpoint(&b1);
    let left1 = route1 - amount; // change carried to out1 (win payout also there)
    // NOTE: both bets must land in ONE block. Build b2 spending out1, then put
    // both txs in a single block at height h.
    let (mut b2, _route2) = forced_win(h, out1, left1 + payout_total(amount, target, E), target, amount, true);

    // Merge: index b1's roll tx and b2's roll tx in the SAME block.
    let tx1 = b1.txdata[1].clone();
    let tx2 = b2.txdata[1].clone();
    b2.txdata = vec![b2.txdata[0].clone(), tx1, tx2];
    index_block(&b2, h)?;

    let stats = call_view(VIEW_STATS);
    assert_eq!(stats[0], 2, "both bets recorded in one block");
    // Bank moved by two wins; still solvent and positive.
    assert!(call_view(VIEW_BANK)[0] > 0, "bank survived two same-block wins");
    // Sanity: two max-win exposures fit the block budget with room to spare.
    let cap = max_bet(bank, target, E);
    let one_win_exposure = payout_total(cap, target, E) - cap;
    assert!(2 * one_win_exposure < block_budget(bank), "two max wins fit the 20% budget");
    test_log!("[SCENARIO] two same-block wins OK, bank={}", call_view(VIEW_BANK)[0]);
    Ok(())
}

/// Full LP exit then revival: deposit, withdraw ALL shares (bank → virtual
/// dust), confirm the table still functions (a bet is honored), then a fresh
/// deposit brings the bank back and mints at the reset price.
#[test]
fn test_full_withdraw_then_revive() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;

    // Deposit.
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

    // Withdraw ALL shares. The LP receives 98% (2% exit fee → /fee_accum).
    let gross = withdraw_payout(total_shares, amount, total_shares);
    let payout = gross - gross * 200 / 10_000;
    let wd = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_WITHDRAW, total_shares, 0] },
        dep_out,
        game_id(),
        total_shares,
    );
    index_block(&wd, h)?;
    h += 1;
    let bank_after = call_view(VIEW_BANK)[0];
    assert!(bank_after < amount / 1_000_000 + 2, "bank drained to virtual dust: {}", bank_after);
    assert_eq!(call_view(VIEW_BANK)[1], 0, "all shares burned");
    let wd_out = op_outpoint(&wd);
    // dep_out also carried the deposit's leftover DIESEL (minted − amount),
    // which rides to the withdraw output alongside the pro-rata payout.
    let leftover = minted - amount;
    assert_eq!(diesel_at(&wd_out), payout + leftover, "LP got the pro-rata payout back");

    // A drained table is DORMANT, by design: with bank ≈ 0 the Kelly cap is
    // 0, so any bet clamps below min_bet and REVERTS (you can't bet against
    // an empty bank). Prove it — nothing is recorded.
    let (dormant, _r) = build_roll(h, wd_out, payout, 32_768, MIN_BET);
    index_block(&dormant, h)?;
    let dormant_out = op_outpoint(&dormant);
    h += 1;
    assert_eq!(call_view(VIEW_STATS)[0], 0, "no bet lands on a drained bank");

    // Revive: a fresh deposit refills the bank and mints at the reset ratio.
    let revive_amt = 50_000_000u128;
    let bank_now = call_view(VIEW_BANK)[0];
    let shares_now = call_view(VIEW_BANK)[1];
    let expect_mint = shares_for_deposit(revive_amt, bank_now, shares_now);
    let revive = create_op_block_with_token_input(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_DEPOSIT, revive_amt] },
        dormant_out, // the bet was refunded here
        DIESEL_ID,
        revive_amt,
    );
    index_block(&revive, h)?;
    let revive_out = op_outpoint(&revive);
    assert_eq!(shares_at(&revive_out), expect_mint, "revive mints at live price");
    assert_eq!(call_view(VIEW_BANK)[0], bank_now + revive_amt, "bank refilled");
    h += 1;

    // ...and now the table plays again: a bet is accepted.
    let (roll2, _r2) = build_roll(h, revive_out, revive_amt, 32_768, 100_000);
    index_block(&roll2, h)?;
    assert_eq!(call_view(VIEW_STATS)[0], 1, "revived table accepts bets again");
    test_log!("[SCENARIO] withdraw → dormant → revive → play OK");
    Ok(())
}

/// Chain rolls inside ONE block: each bet is the largest the caps allow at that
/// moment (so nothing clamps), outcomes are forced per `plan` (want_win, target),
/// and bank / budget are tracked with the math crate exactly as the contract
/// does. Stops when the caps leave less than a min bet. Indexes the block and
/// checks the contract agrees. Returns (bank at block start, bank at block end,
/// net wins paid in the block).
fn one_block_of_rolls(bank0: u128, plan: &[(bool, u128)]) -> Result<(u128, u128, u128)> {
    let (mut input, mut avail, h) = setup_bank(bank0)?;
    let (mut bank, mut paid, mut net_wins) = (bank0, 0u128, 0u128);
    let mut txs = Vec::new();
    let mut last: Option<Block> = None;
    for &(want, target) in plan {
        let allowed = max_net_profit(bank, E).min(block_budget(bank).saturating_sub(paid));
        let bet = max_amount_for_exposure(allowed, target, E);
        if bet < MIN_BET {
            break;
        }
        let (b, _) = forced_win(h, input, avail, target, bet, want);
        if want {
            let payout = payout_total(bet, target, E);
            bank = bank + bet - payout;
            paid += payout - bet;
            net_wins += payout - bet;
            avail = avail - bet + payout;
        } else {
            bank += bet;
            avail -= bet;
        }
        input = OutPoint { txid: b.txdata[1].compute_txid(), vout: 0 };
        txs.push(b.txdata[1].clone());
        last = Some(b);
    }
    // every tx was built against the same height-h header, so they share one block hash
    let mut block = last.expect("at least one roll");
    let coinbase = block.txdata[0].clone();
    block.txdata = std::iter::once(coinbase).chain(txs.iter().cloned()).collect();
    index_block(&block, h)?;
    assert_eq!(call_view(VIEW_STATS)[0], txs.len() as u128, "every roll in the block settled");
    assert_eq!(call_view(VIEW_BANK)[0], bank, "the contract's bank matches the math");
    Ok((bank0, bank, net_wins))
}

/// Max wins back to back in one block until the per-block budget runs out: the
/// bank ends at no less than 80% of where the block started — the bound is tight.
#[test]
fn test_block_of_max_wins_keeps_80_percent_of_the_bank() -> Result<()> {
    let plan = vec![(true, 32_768u128); 40];
    let (b0, b1, won) = one_block_of_rolls(20_000_000, &plan)?;
    assert!(b1 * 10 >= b0 * 8, "bank fell below 80% of the block start: {} → {}", b0, b1);
    assert!(won * 10 <= b0 * 2, "net wins passed 20% with no losses in the block");
    assert!(b1 * 100 < b0 * 85, "the budget should be close to exhausted (tight bound)");
    test_log!("[SCENARIO] max wins in one block: bank {} → {} (won {})", b0, b1, won);
    Ok(())
}

/// Wins, then a big loss, then wins, all in one block. The budget is recomputed
/// from the grown bank, so net wins in the block exceed 20% of the start bank
/// (external audit, 2026-09-30) — paid only out of the same-block loss: the bank
/// still ends above 80% of its start.
#[test]
fn test_losses_in_the_block_fund_extra_wins_but_the_bank_keeps_80_percent() -> Result<()> {
    let mut plan = vec![(true, 32_768u128), (true, 32_768)];
    plan.push((false, t_max(E))); // a ~98% bet sized to the cap: roughly the whole bank, lost
    plan.extend(std::iter::repeat((true, 32_768u128)).take(60));
    let (b0, b1, won) = one_block_of_rolls(20_000_000, &plan)?;
    assert!(won * 10 > b0 * 2, "net wins should pass 20% of the start bank here: {} of {}", won, b0);
    assert!(b1 * 10 >= b0 * 8, "bank fell below 80% of the block start: {} → {}", b0, b1);
    test_log!("[SCENARIO] loss-funded wins in one block: bank {} → {} (won {})", b0, b1, won);
    Ok(())
}

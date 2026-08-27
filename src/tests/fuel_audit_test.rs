//! Fuel audit — the EXACT `gas_used` each opcode consumes, measured with
//! `alkanes::view::simulate_parcel` against committed state (not tx-capped).
//! Every opcode must fit the WORST-CASE per-tx floor `MINIMUM_FUEL_CHANGE1 =
//! 3_500_000` (on mainnet a tx gets `max(3.5M, block_fuel·txsize/blocksize)`,
//! so 3.5M is the floor to clear).
//!
//! The load-bearing check is **CreateTable**: it clones the whole ~260 KB
//! game WASM (`[6, own_tx]`), runs the clone's init, makes two identity
//! staticcalls to the token (99/100), and sub-calls the clone's Deposit — by
//! far the heaviest path. If anything blows the budget, it is this.

use crate::tests::foundation::{deploy_cheekyd, game_id, mint_diesel, GAME_TX, MIN_BET};
use crate::tests::helpers::{
    clear_test_environment, create_op_block_with_token_input, create_operation_block, index_block,
    AlkaneId, Cellpack, DIESEL_ID,
};
use anyhow::Result;
use protorune_support::rune_transfer::RuneTransfer;

/// Worst-case per-tx fuel floor. Every op must fit here.
const BUDGET: u64 = 3_500_000;
/// Generous budget for the simulator so ops always run to completion.
const MEASURE_FUEL: u64 = 1_000_000_000;

fn rune(id: AlkaneId, value: u128) -> RuneTransfer {
    RuneTransfer { id: id.into(), value }
}

fn measure(target: &AlkaneId, inputs: Vec<u128>, runes: Vec<RuneTransfer>, height: u64) -> Result<u64> {
    let mut parcel = alkanes::view::plain_parcel_from_cellpack(Cellpack {
        target: target.clone(),
        inputs,
    });
    parcel.runes = runes;
    parcel.height = height;
    let (_resp, gas_used) = alkanes::view::simulate_parcel(&parcel, MEASURE_FUEL)?;
    Ok(gas_used)
}

fn pct(gas: u64) -> f64 {
    100.0 * gas as f64 / BUDGET as f64
}

fn print_table(rows: &[(&str, u64)]) {
    println!("\n┌─ CheekyD fuel audit ");
    println!("│ {:<28} {:>12} {:>10}  {}", "opcode", "fuel", "% of 3.5M", "fits tx?");
    println!("│ {:-<28} {:->12} {:->10}  {:-<8}", "", "", "", "");
    for (name, gas) in rows {
        println!(
            "│ {:<28} {:>12} {:>9.2}%  {}",
            name, gas, pct(*gas),
            if *gas <= BUDGET { "yes" } else { "NO — EXCEEDS" }
        );
    }
    println!("└──────────────────────────────────────────────────────────────");
}

#[test]
fn fuel_audit_all_opcodes() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheekyd(MIN_BET)?;
    let mut h = deploy.last_height;
    let g = game_id();

    // A REAL 99/100-conforming token for CreateTable: a WILD clone of the
    // game itself (its shares are a fungible alkane with the wallet
    // convention). This makes the identity staticcalls hit real contract
    // code, i.e. a realistic worst case.
    let wild_block = create_operation_block(
        h,
        Cellpack {
            target: AlkaneId { block: 6, tx: GAME_TX },
            inputs: vec![0, MIN_BET, DIESEL_ID.block, DIESEL_ID.tx, 0, 0],
        },
        None,
    );
    index_block(&wild_block, h)?;
    h += 1;
    // Find the WILD clone (first block-2 instance that answers ViewConfig).
    let mut wild = AlkaneId { block: 0, tx: 0 };
    for tx in 0..16u128 {
        let id = AlkaneId { block: 2, tx };
        if alkanes::view::call_view(&id, &vec![103u128], MEASURE_FUEL)
            .map(|v| v.len() == 96)
            .unwrap_or(false)
        {
            wild = id;
            break;
        }
    }
    assert_ne!(wild, AlkaneId { block: 0, tx: 0 }, "WILD clone not found");

    // Commit a real bank so Roll has a non-zero Kelly cap to measure against.
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;
    let dep = create_op_block_with_token_input(
        h,
        Cellpack { target: g.clone(), inputs: vec![2, 250_000_000] },
        diesel_op,
        DIESEL_ID,
        minted,
    );
    index_block(&dep, h)?;
    let _ = h;

    let seed = 1_000_000u128;
    let bet = 1_000_000u128;

    let mut rows: Vec<(&str, u64)> = Vec::new();
    // Views
    rows.push(("ViewBank (101)", measure(&g, vec![101], vec![], 0)?));
    rows.push(("ViewStats (102)", measure(&g, vec![102], vec![], 0)?));
    rows.push(("ViewConfig (103)", measure(&g, vec![103], vec![], 0)?));
    rows.push(("ViewMaxBet (104)", measure(&g, vec![104, 32_768], vec![], 0)?));
    rows.push(("ViewTableOf (105)", measure(&g, vec![105, wild.block, wild.tx], vec![], 0)?));
    // Mutating ops (measured, not committed)
    rows.push(("Deposit (2)", measure(&g, vec![2, seed], vec![rune(DIESEL_ID, seed)], 0)?));
    rows.push(("Donate (5)", measure(&g, vec![5, seed], vec![rune(DIESEL_ID, seed)], 0)?));
    rows.push((
        "Roll (1)",
        measure(&g, vec![1, 32_768, bet, 0], vec![rune(DIESEL_ID, bet)], 880_050)?,
    ));
    // THE HEAVY ONE: clone + init + 2 staticcalls + sub-deposit.
    rows.push((
        "CreateTable (4)",
        measure(
            &g,
            vec![4, wild.block, wild.tx],
            vec![rune(wild.clone(), seed)],
            880_050,
        )?,
    ));

    print_table(&rows);

    for (name, gas) in &rows {
        assert!(*gas <= BUDGET, "{name} uses {gas} fuel > {BUDGET} budget");
    }
    Ok(())
}

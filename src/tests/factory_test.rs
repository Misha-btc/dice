//! Factory pattern (PERMAFROST-style): the deployed WASM is a template —
//! anyone can clone it via `[6, TEMPLATE_TX, 0, ...]` into an independent
//! game instance for any token, with its own bank, shares and wallet
//! identity. No bytecode re-deploy.

use crate::test_log;
use crate::tests::foundation::{
    deploy_cheeky_bricky, game_id, mint_diesel, GAME_TX, MIN_BET,
};
use crate::tests::helpers::{
    clear_test_environment, create_op_block_with_token_input, create_operation_block, index_block,
    AlkaneId, Cellpack, DIESEL_ID,
};
use anyhow::Result;

const OP_DEPOSIT: u128 = 2;
const VIEW_BANK: u128 = 101;
const FUEL: u64 = 100_000_000;

/// Truncate to 16 bytes on a char boundary — mirrors the contract's
/// pack_str limit for derived identities.
fn trunc16(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if out.len() + ch.len_utf8() > 16 {
            break;
        }
        out.push(ch);
    }
    out
}

/// Encode a string as the standard u128 LE token-name packing.
fn pack(s: &str) -> u128 {
    let mut b = [0u8; 16];
    b[..s.len()].copy_from_slice(s.as_bytes());
    u128::from_le_bytes(b)
}

fn view_bytes(id: &AlkaneId, inputs: Vec<u128>) -> Vec<u8> {
    alkanes::view::call_view(id, &inputs, FUEL).unwrap_or_default()
}

fn first_u128(bytes: &[u8]) -> u128 {
    if bytes.len() < 16 {
        return 0;
    }
    u128::from_le_bytes(bytes[..16].try_into().unwrap())
}

const OP_CREATE_TABLE: u128 = 4;
const VIEW_TABLE_OF: u128 = 105;

/// ViewTableOf on `host` → AlkaneId (zeros = no table).
fn table_of(host: &AlkaneId, token: &AlkaneId) -> AlkaneId {
    let b = view_bytes(host, vec![VIEW_TABLE_OF, token.block, token.tx]);
    AlkaneId {
        block: first_u128(&b),
        tx: if b.len() >= 32 { first_u128(&b[16..]) } else { 0 },
    }
}

#[test]
fn test_table_registry_one_table_per_token_forever() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;
    let zero = AlkaneId { block: 0, tx: 0 };

    // The template registered ITSELF as the table for its own token.
    assert_eq!(table_of(&game_id(), &DIESEL_ID), game_id(), "self-registration");

    // A 99/100-conforming token to register a table for: a WILD clone of the
    // game itself (its shares are a fungible alkane with the wallet
    // convention). No funding needed — CreateTable is not payable/seeded.
    let wild_block = create_operation_block(
        h,
        Cellpack {
            target: AlkaneId { block: 6, tx: GAME_TX },
            inputs: vec![0, DIESEL_ID.block, DIESEL_ID.tx, pack("WILD"), pack("WLD")],
        },
        None,
    );
    index_block(&wild_block, h)?;
    h += 1;
    let mut wild = zero;
    for tx in 0..16u128 {
        let id = AlkaneId { block: 2, tx };
        if view_bytes(&id, vec![99]) == b"WILD".to_vec() {
            wild = id;
            break;
        }
    }
    assert_ne!(wild, zero, "wildcat token not found");
    assert_eq!(table_of(&game_id(), &wild), zero, "no table yet");

    // Create the canonical table for WILD — no seed attached.
    let create = create_operation_block(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_CREATE_TABLE, wild.block, wild.tx] },
        None,
    );
    index_block(&create, h)?;
    h += 1;

    let table = table_of(&game_id(), &wild);
    assert_eq!(table.block, 2, "table is a block-2 clone");
    assert_ne!(table.tx, 0);
    test_log!("[REGISTRY] WILD table at {:?}", table);

    // Identity derived from the token's own 99/100.
    assert_eq!(view_bytes(&table, vec![99]), b"WILD House".to_vec(), "derived name");
    assert_eq!(view_bytes(&table, vec![100]), b"hWLD".to_vec(), "derived symbol");

    // Born EMPTY: bank 0, no shares — dormant until someone deposits.
    assert_eq!(first_u128(&view_bytes(&table, vec![101])), 0, "bank starts empty");
    assert_eq!(first_u128(&view_bytes(&table, vec![101])[16..]), 0, "no shares yet");

    // Second creation for the same token reverts; the original id survives.
    let again = create_operation_block(
        h,
        Cellpack { target: game_id(), inputs: vec![OP_CREATE_TABLE, wild.block, wild.tx] },
        None,
    );
    index_block(&again, h)?;
    h += 1;
    assert_eq!(table_of(&game_id(), &wild), table, "one table per token, forever");

    // CreateTable on a CLONE reverts — the registry lives on the block-4
    // template only.
    let wumbo = AlkaneId { block: 4, tx: 0x777 };
    let on_clone = create_operation_block(
        h,
        Cellpack { target: table, inputs: vec![OP_CREATE_TABLE, wumbo.block, wumbo.tx] },
        None,
    );
    index_block(&on_clone, h)?;
    assert_eq!(table_of(&game_id(), &wumbo), zero, "clone cannot mint registry entries");
    Ok(())
}

#[test]
fn test_reinit_is_rejected() -> Result<()> {
    // The /initialized marker makes init one-shot: a second init call with
    // different params must revert and change nothing.
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let h = deploy.last_height;

    let attack = create_operation_block(
        h,
        Cellpack {
            target: game_id(),
            // attacker tries to re-point the game at another token with a
            // different min_bet and their own branding
            inputs: vec![0, 4, 0x999, pack("EVIL"), pack("EVL")],
        },
        None,
    );
    index_block(&attack, h)?;

    let cfg = view_bytes(&game_id(), vec![103]);
    assert_eq!(first_u128(&cfg), MIN_BET, "min_bet unchanged");
    assert_eq!(first_u128(&cfg[64..]), DIESEL_ID.block, "bet token unchanged");
    assert_eq!(first_u128(&cfg[80..]), DIESEL_ID.tx, "bet token unchanged");
    assert_eq!(view_bytes(&game_id(), vec![99]), b"Cheeky Bricky".to_vec(), "name unchanged");
    Ok(())
}

#[test]
fn test_factory_clone_spins_an_independent_game() -> Result<()> {
    clear_test_environment();
    let deploy = deploy_cheeky_bricky()?;
    let mut h = deploy.last_height;

    // Clone the template via the factory (block 6) with its own identity.
    // (The bet token is DIESEL again — the only harness-mintable token — but
    // the clone still gets its own independent bank/shares/auth.)
    let clone_block = create_operation_block(
        h,
        Cellpack {
            target: AlkaneId { block: 6, tx: GAME_TX },
            inputs: vec![0, DIESEL_ID.block, DIESEL_ID.tx, pack("FROSTY"), pack("FRT")],
        },
        None,
    );
    index_block(&clone_block, h)?;
    h += 1;

    // Find the clone among block-2 instances by its custom name (the other
    // block-2 residents are auth-token clones with "AUTH …" names).
    let mut clone = None;
    for tx in 0..16u128 {
        let id = AlkaneId { block: 2, tx };
        if view_bytes(&id, vec![99]) == b"FROSTY".to_vec() {
            clone = Some(id);
            break;
        }
    }
    let clone = clone.expect("factory clone not found by its name");
    test_log!("[FACTORY] clone found at {:?}", clone);
    assert_eq!(view_bytes(&clone, vec![100]), b"FRT".to_vec(), "clone symbol");

    // The template keeps its default identity — clones are tellable apart.
    assert_eq!(view_bytes(&game_id(), vec![99]), b"Cheeky Bricky".to_vec());
    assert_eq!(view_bytes(&game_id(), vec![100]), b"CBRK".to_vec());

    // Deposit into the CLONE only: its bank grows, the template's stays 0.
    let (minted, diesel_op) = mint_diesel(h)?;
    h += 1;
    let amount = 100_000_000u128;
    let dep = create_op_block_with_token_input(
        h,
        Cellpack { target: clone, inputs: vec![OP_DEPOSIT, amount] },
        diesel_op,
        DIESEL_ID,
        minted,
    );
    index_block(&dep, h)?;

    assert_eq!(
        first_u128(&view_bytes(&clone, vec![VIEW_BANK])),
        amount,
        "clone bank grew"
    );
    assert_eq!(
        first_u128(&view_bytes(&game_id(), vec![VIEW_BANK])),
        0,
        "template bank untouched — instances are fully independent"
    );

    // The clone's config is fully populated and matches the template's
    // immutable constants (same bytecode, own storage).
    let clone_cfg = view_bytes(&clone, vec![103]);
    assert_eq!(clone_cfg.len(), 112, "clone ViewConfig shape");
    assert_eq!(first_u128(&clone_cfg[16..]), 100, "clone edge is the fixed 1%");
    assert_eq!(first_u128(&clone_cfg[96..]), 200, "clone exit-fee ceiling is 2%");

    // This was a RAW clone (not via CreateTable): it works, but it is a
    // "wildcat" — the canonical registry still points DIESEL at the
    // template, so ecosystem frontends never see this instance.
    assert_eq!(table_of(&game_id(), &DIESEL_ID), game_id(), "registry untouched by a raw clone");
    Ok(())
}

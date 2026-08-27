//! Test helpers for Fireball — adapted from fire/src/tests/helpers.rs.
//! Minimal subset needed for cellpack-based contract tests.

pub use crate::test_log;
pub use alkanes::indexer::index_block;
pub use alkanes::view;
pub use alkanes_support::cellpack::Cellpack;
pub use alkanes_support::envelope::RawEnvelope;
pub use alkanes_support::id::AlkaneId;
pub use anyhow::Result;
pub use bitcoin::blockdata::transaction::OutPoint;
pub use bitcoin::{Address, Block, Transaction};
pub use bitcoin::{Amount, ScriptBuf, Sequence, TxIn, TxOut, Witness};
pub use metashrew_core::index_pointer::AtomicPointer;
pub use ordinals::{Etching, Rune, Runestone};
pub use protorune::protostone::Protostones;
pub use protorune::test_helpers::{create_block_with_coinbase_tx, get_address, ADDRESS1};
pub use protorune_support::balance_sheet::BalanceSheet;
pub use protorune_support::protostone::{Protostone, ProtostoneEdict};
pub use protorune_support::balance_sheet::ProtoruneRuneId;
pub use protorune_support::network::{set_network, NetworkParams};

/// DIESEL token id on Alkanes mainnet.
pub const DIESEL_ID: AlkaneId = AlkaneId { block: 2, tx: 0 };

pub fn configure_network() {
    set_network(NetworkParams {
        bech32_prefix: String::from("bcrt"),
        p2pkh_prefix: 0x64,
        p2sh_prefix: 0xc4,
    });
}

pub fn clear_test_environment() {
    metashrew_core::clear();
    configure_network();
    for height in 0..3 {
        let block = create_block_with_coinbase_tx(height);
        index_block(&block, height).expect("failed to index empty block");
    }
}

pub fn get_sheet_by_outpoint(
    outpoint: &OutPoint,
) -> Result<BalanceSheet<metashrew_core::index_pointer::IndexPointer>> {
    use protorune::balance_sheet::load_sheet;
    use protorune::tables::RuneTable;
    use alkanes::message::AlkaneMessageContext;
    use protorune::message::MessageContext;
    use metashrew_support::index_pointer::KeyValuePointer;
    use metashrew_support::utils::consensus_encode;

    let ptr = RuneTable::for_protocol(AlkaneMessageContext::protocol_tag())
        .OUTPOINT_TO_RUNES
        .select(&consensus_encode(outpoint)?);
    Ok(load_sheet(&ptr))
}

pub fn create_multiple_cellpack_with_witness_and_txins_edicts(
    cellpacks: Vec<Cellpack>,
    txins: Vec<TxIn>,
    etch: bool,
    edicts: Vec<ProtostoneEdict>,
) -> Transaction {
    let protocol_id = 1;
    let protostones: Vec<Protostone> = cellpacks
        .into_iter()
        .map(|cellpack| Protostone {
            message: cellpack.encipher(),
            pointer: Some(0),
            refund: Some(0),
            edicts: edicts.clone(),
            from: None,
            burn: None,
            protocol_tag: protocol_id,
        })
        .collect();

    let etching = if etch {
        Some(Etching {
            rune: Some(Rune(0)),
            divisibility: Some(0),
            premine: Some(21_000_000),
            spacers: Some(0),
            symbol: Some('X'),
            terms: None,
            turbo: false,
        })
    } else {
        None
    };

    let runestone: ScriptBuf = (Runestone {
        etching,
        pointer: Some(0),
        edicts: Vec::new(),
        mint: None,
        protocol: protostones.encipher().ok(),
    })
    .encipher();

    let address: bitcoin::address::Address<bitcoin::address::NetworkChecked> =
        get_address(&ADDRESS1().as_str());
    let op_return = TxOut {
        value: Amount::from_sat(0),
        script_pubkey: runestone,
    };
    let recipient_output = TxOut {
        value: Amount::from_sat(100_000_000),
        script_pubkey: address.script_pubkey(),
    };

    let inputs = if txins.is_empty() {
        vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }]
    } else {
        txins
    };

    Transaction {
        version: bitcoin::blockdata::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: inputs,
        output: vec![recipient_output, op_return],
    }
}

pub fn create_multiple_cellpack_with_witness(
    witness: Witness,
    cellpacks: Vec<Cellpack>,
    etch: bool,
) -> Transaction {
    let txin = TxIn {
        previous_output: OutPoint::null(),
        script_sig: ScriptBuf::new(),
        sequence: Sequence::MAX,
        witness,
    };
    create_multiple_cellpack_with_witness_and_txins_edicts(cellpacks, vec![txin], etch, vec![])
}

pub fn create_multiple_cellpack_with_witness_and_in(
    witness: Witness,
    cellpacks: Vec<Cellpack>,
    previous_output: OutPoint,
    etch: bool,
) -> Transaction {
    let txin = TxIn {
        previous_output,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::MAX,
        witness,
    };
    create_multiple_cellpack_with_witness_and_txins_edicts(cellpacks, vec![txin], etch, vec![])
}

/// Deploy + initialize a series of contracts in one block. Each pair is
/// (wasm_bytes, init_cellpack). Subsequent txs chain via the previous
/// tx's vout=0 so contracts can hand off context.
pub fn init_with_multiple_cellpacks_with_tx(
    binaries: Vec<Vec<u8>>,
    cellpacks: Vec<Cellpack>,
) -> Block {
    let block_height = 880_000;
    let mut test_block = create_block_with_coinbase_tx(block_height);
    let mut previous_out: Option<OutPoint> = None;

    let mut txs = binaries
        .into_iter()
        .zip(cellpacks.into_iter())
        .map(|(binary, cellpack)| {
            let witness = if binary.is_empty() {
                Witness::new()
            } else {
                RawEnvelope::from(binary).to_witness(true)
            };

            if let Some(previous_output) = previous_out {
                let tx = create_multiple_cellpack_with_witness_and_in(
                    witness,
                    vec![cellpack],
                    previous_output,
                    false,
                );
                previous_out = Some(OutPoint {
                    txid: tx.compute_txid(),
                    vout: 0,
                });
                tx
            } else {
                let tx = create_multiple_cellpack_with_witness(witness, vec![cellpack], false);
                previous_out = Some(OutPoint {
                    txid: tx.compute_txid(),
                    vout: 0,
                });
                tx
            }
        })
        .collect::<Vec<Transaction>>();

    test_block.txdata.append(&mut txs);
    test_block
}

/// Deploy a contract by including its WASM in the witness. The cellpack
/// `target` should normally be `{block: 3, tx: TX}` — the deployed
/// contract will then live at `{block: 4, tx: TX}`. inputs = vec![] means
/// "bytecode only, initialize later".
pub fn create_deployment_block(height: u32, wasm_bytes: &[u8], cellpack: Cellpack) -> Block {
    let mut block = create_block_with_coinbase_tx(height);
    let witness = RawEnvelope::from(wasm_bytes.to_vec()).to_witness(true);
    let tx = create_multiple_cellpack_with_witness(witness, vec![cellpack], false);
    block.txdata.push(tx);
    block
}

/// Build a tx that spends `input_outpoint` (carrying some alkane) and
/// edicts `amount` of `token_id` to output 0 (where the protostone
/// executes the cellpack). This is the proper way to route an alkane
/// into a contract call.
pub fn create_op_block_with_token_input(
    height: u32,
    cellpack: Cellpack,
    input_outpoint: OutPoint,
    token_id: AlkaneId,
    amount: u128,
) -> Block {
    let mut block = create_block_with_coinbase_tx(height);
    let edicts = vec![ProtostoneEdict {
        id: ProtoruneRuneId {
            block: token_id.block,
            tx: token_id.tx,
        },
        amount,
        output: 0,
    }];
    let txins = vec![TxIn {
        previous_output: input_outpoint,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::MAX,
        witness: Witness::new(),
    }];
    let tx = create_multiple_cellpack_with_witness_and_txins_edicts(
        vec![cellpack],
        txins,
        false,
        edicts,
    );
    block.txdata.push(tx);
    block
}

/// Like `create_op_block_with_token_input` but routes TWO token edicts, both
/// pulled from the same `input_outpoint`, into one call. Used to exercise the
/// "hand the foreign token back alongside the result" path (e.g. redeem shares
/// while a stray DIESEL rides along).
pub fn create_op_block_with_two_token_inputs(
    height: u32,
    cellpack: Cellpack,
    input_outpoint: OutPoint,
    id_a: AlkaneId,
    amount_a: u128,
    id_b: AlkaneId,
    amount_b: u128,
) -> Block {
    let mut block = create_block_with_coinbase_tx(height);
    let edicts = vec![
        ProtostoneEdict {
            id: ProtoruneRuneId { block: id_a.block, tx: id_a.tx },
            amount: amount_a,
            output: 0,
        },
        ProtostoneEdict {
            id: ProtoruneRuneId { block: id_b.block, tx: id_b.tx },
            amount: amount_b,
            output: 0,
        },
    ];
    let txins = vec![TxIn {
        previous_output: input_outpoint,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::MAX,
        witness: Witness::new(),
    }];
    let tx = create_multiple_cellpack_with_witness_and_txins_edicts(
        vec![cellpack],
        txins,
        false,
        edicts,
    );
    block.txdata.push(tx);
    block
}

pub fn create_operation_block(
    height: u32,
    cellpack: Cellpack,
    input_outpoint: Option<OutPoint>,
) -> Block {
    let mut block = create_block_with_coinbase_tx(height);
    let tx = if let Some(outpoint) = input_outpoint {
        create_multiple_cellpack_with_witness_and_in(
            Witness::new(),
            vec![cellpack],
            outpoint,
            false,
        )
    } else {
        create_multiple_cellpack_with_witness(Witness::new(), vec![cellpack], false)
    };
    block.txdata.push(tx);
    block
}

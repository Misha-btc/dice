//! Cheeky Bricky Operator — the fee-withdrawal auth key.
//!
//! Minimal and immutable. It holds NO funds. `Init` mints one key unit (units
//! of itself) to the deployer; every game table's `WithdrawFees` requires this
//! key — its id is hardcoded into the game as `OPERATOR_AUTH_ID` — before it
//! releases that table's accrued 2% LP-exit fees. Whoever holds the key is the
//! operator; the key is spent-and-returned on each withdrawal (never consumed).
//! Implements the 99/100 wallet convention so the key shows up cleanly in the
//! operator's wallet. Deploy it at the hardcoded slot BEFORE the game.

use alkanes_runtime::{declare_alkane, message::MessageDispatch, runtime::AlkaneResponder, storage::StoragePointer};
use alkanes_support::{parcel::AlkaneTransfer, response::CallResponse};
use anyhow::{anyhow, Result};
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;
use std::sync::Arc;

#[derive(MessageDispatch)]
pub enum OperatorMessage {
    /// One-time. Mints 1 key unit (units of self) to the deployer.
    #[opcode(0)]
    Init,

    /// Wallet convention — the key's display name.
    #[opcode(99)]
    GetName,

    /// Wallet convention — the key's display symbol.
    #[opcode(100)]
    GetSymbol,
}

fn ptr(k: &str) -> StoragePointer {
    StoragePointer::from_keyword(k)
}

#[derive(Default)]
pub struct Operator(());

impl Operator {
    fn init(&self) -> Result<CallResponse> {
        if !ptr("/initialized").get().is_empty() {
            return Err(anyhow!("already initialized"));
        }
        ptr("/initialized").set(Arc::new(vec![1]));
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        // The auth key: one unit of this contract, to the deployer's output.
        response.alkanes.0.push(AlkaneTransfer { id: context.myself.clone(), value: 1 });
        Ok(response)
    }

    fn get_name(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = b"Cheeky Bricky Operator".to_vec();
        Ok(response)
    }

    fn get_symbol(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = b"CBOP".to_vec();
        Ok(response)
    }
}

impl AlkaneResponder for Operator {}

declare_alkane! {
    impl AlkaneResponder for Operator {
        type Message = OperatorMessage;
    }
}

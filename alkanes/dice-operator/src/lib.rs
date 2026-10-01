//! Dice Operator — the fee-withdrawal auth key.
//!
//! Minimal and immutable. It holds NO funds. `Init` mints `KEY_SUPPLY` (3)
//! interchangeable key units (units of itself) to the deployer; every game
//! table's `WithdrawFees`/`SetExitFee` requires one of them — the id is
//! hardcoded into the game as `OPERATOR_AUTH_ID` — before it acts. Whoever
//! holds a unit is the operator: three are minted so the spares can live in
//! separate wallets as backups (any single one is full operator power). A key
//! is spent-and-returned on each call (never consumed). Implements the 99/100
//! wallet convention so the key shows up cleanly in the operator's wallet.
//! Deploy it at the hardcoded slot BEFORE the game.

use alkanes_runtime::{declare_alkane, message::MessageDispatch, runtime::AlkaneResponder, storage::StoragePointer};
use alkanes_support::{parcel::AlkaneTransfer, response::CallResponse};
use anyhow::{anyhow, Result};
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;
use std::sync::Arc;

/// Key units minted once at init: one to use, two spares to keep apart. Any
/// single unit authorizes, and none can ever be minted again.
pub const KEY_SUPPLY: u128 = 3;

#[derive(MessageDispatch)]
pub enum OperatorMessage {
    /// One-time. Mints `KEY_SUPPLY` (3) key units (units of self) to the
    /// deployer.
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
        // The auth keys: KEY_SUPPLY units of this contract, to the deployer's output.
        response.alkanes.0.push(AlkaneTransfer { id: context.myself.clone(), value: KEY_SUPPLY });
        Ok(response)
    }

    fn get_name(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = b"Dice Operator".to_vec();
        Ok(response)
    }

    fn get_symbol(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = b"DOP".to_vec();
        Ok(response)
    }
}

impl AlkaneResponder for Operator {}

declare_alkane! {
    impl AlkaneResponder for Operator {
        type Message = OperatorMessage;
    }
}

//! Cheeky Bricky Treasury — the operator's fee collector.
//!
//! The game is immutable and has no privileged party; it accrues a 2% LP-exit
//! fee and permissionlessly `SweepFees` it here. This contract just holds
//! whatever it's sent (in any token) and lets its OWNER withdraw. Ownership is
//! a single unit of this contract itself, minted to the deployer at init and
//! required (spent-and-returned) to withdraw — the std-proxy pattern, so no
//! external auth-token factory is needed.

use alkanes_runtime::{declare_alkane, message::MessageDispatch, runtime::AlkaneResponder, storage::StoragePointer};
use alkanes_support::{id::AlkaneId, parcel::AlkaneTransfer, response::CallResponse};
use anyhow::{anyhow, Result};
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;
use std::sync::Arc;

#[derive(MessageDispatch)]
pub enum TreasuryMessage {
    /// One-time. Mints 1 owner-key unit (units of self) to the deployer.
    #[opcode(0)]
    Init,

    /// Permissionless: accept and HOLD any incoming alkanes (the game's fee
    /// sweep lands here). Returns nothing — everything sent stays credited to
    /// this contract's balance.
    #[opcode(1)]
    Collect,

    /// Owner-only: send `amount` of token `(token_block, token_tx)` to the
    /// caller. Requires 1 unit of this contract in incoming (the owner key),
    /// which is returned alongside the withdrawal.
    #[opcode(2)]
    Withdraw {
        token_block: u128,
        token_tx: u128,
        amount: u128,
    },
}

fn ptr(k: &str) -> StoragePointer {
    StoragePointer::from_keyword(k)
}

#[derive(Default)]
pub struct Treasury(());

impl Treasury {
    fn init(&self) -> Result<CallResponse> {
        if !ptr("/initialized").get().is_empty() {
            return Err(anyhow!("already initialized"));
        }
        ptr("/initialized").set(Arc::new(vec![1]));
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        // The owner key: one unit of this contract, to the deployer's output.
        response.alkanes.0.push(AlkaneTransfer { id: context.myself.clone(), value: 1 });
        Ok(response)
    }

    fn collect(&self) -> Result<CallResponse> {
        // Keep everything sent (incoming is already credited to our balance);
        // return an empty response so nothing flows back out.
        Ok(CallResponse::default())
    }

    fn withdraw(&self, token_block: u128, token_tx: u128, amount: u128) -> Result<CallResponse> {
        let context = self.context()?;
        // Owner check: at least one unit of self (the key) must be present.
        let key: u128 = context
            .incoming_alkanes
            .0
            .iter()
            .filter(|t| t.id == context.myself)
            .map(|t| t.value)
            .sum();
        if key < 1 {
            return Err(anyhow!("not owner: attach 1 unit of the treasury key"));
        }
        if amount == 0 {
            return Err(anyhow!("amount must be > 0"));
        }
        let token = AlkaneId { block: token_block, tx: token_tx };
        if token == context.myself {
            return Err(anyhow!("cannot withdraw the owner key as a token"));
        }

        let mut response = CallResponse::default();
        // Pay out the requested token, return the owner key, and hand back any
        // other incoming untouched.
        response.alkanes.0.push(AlkaneTransfer { id: token.clone(), value: amount });
        response.alkanes.0.push(AlkaneTransfer { id: context.myself.clone(), value: key });
        for t in context.incoming_alkanes.0.iter() {
            if t.id != context.myself {
                response.alkanes.0.push(t.clone());
            }
        }
        Ok(response)
    }
}

impl AlkaneResponder for Treasury {}

declare_alkane! {
    impl AlkaneResponder for Treasury {
        type Message = TreasuryMessage;
    }
}

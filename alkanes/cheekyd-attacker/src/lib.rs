//! TEST-ONLY fixture — a minimal alkane that wraps a call to CheekyD's Roll.
//!
//! Its whole purpose is to reach `Roll` from a CONTRACT context (so the
//! game sees a non-zero `context.caller`) and prove the flash-attack guard
//! rejects it. Never part of the deployed product.

use alkanes_runtime::{declare_alkane, message::MessageDispatch, runtime::AlkaneResponder};
use alkanes_support::{cellpack::Cellpack, id::AlkaneId, response::CallResponse};
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;

#[derive(MessageDispatch)]
pub enum AttackerMessage {
    /// No-op init (deploy runs opcode 0).
    #[opcode(0)]
    Init,

    /// Forward a Roll to the game at `(game_block, game_tx)` with
    /// `(target, amount, min_amount)`, passing along all incoming alkanes
    /// (the DIESEL bet). Because THIS contract makes the call, the game sees
    /// `caller = this attacker` — which the guard must reject.
    #[opcode(1)]
    AttackRoll {
        game_block: u128,
        game_tx: u128,
        target: u128,
        amount: u128,
        min_amount: u128,
    },
}

#[derive(Default)]
pub struct Attacker(());

impl Attacker {
    fn init(&self) -> Result<CallResponse> {
        let context = self.context()?;
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }

    fn attack_roll(
        &self,
        game_block: u128,
        game_tx: u128,
        target: u128,
        amount: u128,
        min_amount: u128,
    ) -> Result<CallResponse> {
        let context = self.context()?;
        // Forward the bet and call Roll. If the game accepted contract
        // callers, this wrapper could inspect the response and revert on a
        // loss — the flash attack. The guard makes this call fail outright.
        self.call(
            &Cellpack {
                target: AlkaneId { block: game_block, tx: game_tx },
                inputs: vec![1, target, amount, min_amount],
            },
            &context.incoming_alkanes,
            self.fuel(),
        )
    }
}

impl AlkaneResponder for Attacker {}

declare_alkane! {
    impl AlkaneResponder for Attacker {
        type Message = AttackerMessage;
    }
}

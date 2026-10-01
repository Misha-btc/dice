//! Dice — instant roll-under dice on ALKANES (fully immutable: the 1%
//! edge is a compile-time constant, there is no privileged capability of
//! any kind). Thin declare_alkane! wrapper over `DiceBase`. The contract
//! is simultaneously the game singleton and the LP share token (units of
//! itself). The whole edge accrues to the LP bank — no protocol fee.

use alkanes_runtime::{declare_alkane, message::MessageDispatch, runtime::AlkaneResponder};
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;

use dice_game_runtime::DiceBase;

#[derive(MessageDispatch)]
pub enum DiceMessage {
    /// One-time init: the bet/bank token id (mainnet: 2:490) and the share
    /// token's name/symbol (u128-packed LE strings, `0` = "Dice"/"DICE"
    /// defaults) — immutable afterwards, like everything else
    /// (the 1% edge and the 1000 min-bet are compiled in). **Factory-ready**:
    /// clone the deployed template for ANY token via
    /// `[6, TEMPLATE_TX, 0, tb, tt, name, symbol]` — each clone is an
    /// independent game (own bank/shares). Direct deploy:
    /// `[3, TX, 0, ...]` (gotcha: deploy runs opcode 0).
    #[opcode(0)]
    InitDice {
        token_block: u128,
        token_tx: u128,
        name: u128,
        symbol: u128,
    },

    /// The game: bet `amount` of the bet token that this block's roll (from
    /// `sha256(blockhash ‖ txid)`) lands strictly under `target`
    /// (∈ [656, t_max(edge)]; win chance target/65536). Settles atomically —
    /// win pays `amount · (1−edge) · 65536 / target` from the bank onto the
    /// caller's output. An oversized bet is CLAMPED down to what the caps
    /// allow (excess returned as change) instead of reverting; `min_amount`
    /// reverts instead if the clamp falls below it (`0` = accept any size).
    /// `amount = 0` bets EVERYTHING sent — the mint-and-roll flow: chain
    /// `[free mint] → [Roll(target, 0)]` in one tx and the roll takes the
    /// whole mint, whatever size it turns out to be. ONE Roll per transaction
    /// per table: a second Roll in the same tx would share the same roll, so
    /// it reverts (full refund). Payable in the bet
    /// token. Response data: `(roll, win, payout, effective_amount)` 4×u128 LE.
    #[opcode(1)]
    Roll {
        target: u128,
        amount: u128,
        min_amount: u128,
    },

    /// LP in: add exactly `amount` of the bet token to the bank, mint share
    /// units of this contract at the current share price. Overpay returned
    /// as change. Payable in the bet token.
    #[opcode(2)]
    Deposit { amount: u128 },

    /// LP out: burn exactly `shares` for their pro-rata slice of the bank.
    /// Overpaid shares returned as change; `min_payout` is a slippage floor
    /// (`0` disables). Payable in shares (units of this contract).
    #[opcode(3)]
    Withdraw { shares: u128, min_payout: u128 },

    /// Permissionless factory front door: create the CANONICAL table for a
    /// token — exactly once per token, forever. NOT payable / NOT seeded: the
    /// table is born empty (dormant until someone deposits), like the
    /// template's own table; the creator gets no privilege, so squatting an
    /// empty table is pointless and the tx fee is the only spam gate.
    /// Identity is read from the token's own 99/100 ("Brick" → "Brick
    /// House"/"hBRICK"), so a dead id fails here. Returns the table id as
    /// `(block, tx)` 2×u128 LE. Registry lives on the block-4 template only.
    /// Raw `[6,TX,0,…]` clones stay possible but unregistered — invisible to
    /// frontends that resolve via ViewTableOf.
    #[opcode(4)]
    CreateTable {
        token_block: u128,
        token_tx: u128,
    },

    /// Add exactly `amount` of the bet token to the bank WITHOUT minting
    /// shares — a gift to LPs (seeding, marketing). Overpay returned as
    /// change.
    #[opcode(5)]
    Donate { amount: u128 },

    /// Operator-only: withdraw this table's accrued LP-exit fees (in the bet
    /// token) to the caller. Requires the hardcoded operator auth key
    /// (`OPERATOR_AUTH_ID`) in the inputs — spent-and-returned, so any one
    /// key unit drains every table. Reverts on a missing key or a zero balance.
    #[opcode(6)]
    WithdrawFees,

    /// Operator-only: set this table's live LP-exit fee to `bps` (0..=200).
    /// Reverts if it exceeds 200 (the 2% ceiling is immutable — even the
    /// operator can never make exits cost more than 2%). Requires the operator
    /// auth key (returned). `bps = 0` = free exits.
    #[opcode(7)]
    SetExitFee { bps: u128 },

    /// Standard fungible-token name — wallets probe this (shares live in
    /// wallets, so the 99/100 convention is load-bearing).
    #[opcode(99)]
    #[returns(String)]
    GetName,

    /// Standard fungible-token symbol.
    #[opcode(100)]
    #[returns(String)]
    GetSymbol,

    /// View: `(bank, total_shares, fee_accum, exit_bps)` 4×u128 LE.
    #[opcode(101)]
    #[returns(Vec<u8>)]
    ViewBank,

    /// View: `(total_bets, total_wagered, total_won)` 3×u128 LE.
    #[opcode(102)]
    #[returns(Vec<u8>)]
    ViewStats,

    /// View: `(min_bet, edge_bps, t_min, t_max, token_block, token_tx,
    /// exit_bps_max)` 7×u128 LE — immutable game parameters incl. the 2% exit-fee
    /// ceiling (the live fee is ViewBank field 4).
    #[opcode(103)]
    #[returns(Vec<u8>)]
    ViewConfig,

    /// View: `(max_bet, max_net_profit)` for `target` at the current bank
    /// and edge, 2×u128 LE — the frontend's bet-slider limits.
    #[opcode(104)]
    #[returns(Vec<u8>)]
    ViewMaxBet { target: u128 },

    /// View: registry lookup — the canonical table for `(token_block,
    /// token_tx)` as `(block, tx)` 2×u128 LE; zeros if none exists yet.
    #[opcode(105)]
    #[returns(Vec<u8>)]
    ViewTableOf { token_block: u128, token_tx: u128 },
}

#[derive(Default)]
pub struct Dice();

impl DiceBase for Dice {}
impl AlkaneResponder for Dice {}

declare_alkane! {
    impl AlkaneResponder for Dice {
        type Message = DiceMessage;
    }
}

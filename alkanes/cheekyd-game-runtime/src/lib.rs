//! CheekyD runtime — instant roll-under dice vs an LP-owned bankroll.
//!
//! One transaction = one bet = one resolution: the roll is derived from
//! `sha256(blockhash ‖ txid)` of the block that mines the bet (ord#4541's
//! canonical instant-randomness recipe) and the payout lands on the bet tx's
//! own output in the same call. No rounds, no claims, no NFTs, and **no
//! pending liabilities ever** — which is what makes open-ended LP
//! deposit/withdraw safe.
//!
//! The bet/bank currency is a single alkane fixed at init (mainnet: 2:490)
//! and immutable afterwards. There is **no protocol fee and no external
//! sink**: the whole 1% edge (a compile-time constant — no admin, no auth
//! token, no knob of any kind) accrues to the bank — only LPs earn.
//!
//! ## Solvency: half-Kelly per bet, 20% budget per block, clamp not revert
//!
//! A bet's net exposure may not exceed `edge/2 · bank` (half-Kelly — the LP
//! growth guarantee), and a block's aggregate realized net payout may not
//! exceed `20% · bank` (miner block-stuffing defense). A bet that would
//! exceed what remains is **clamped down** to fit (excess returned as
//! change) rather than reverted — the caller can set `min_amount` to revert
//! instead if the clamped size is below their floor. Rounding always favors
//! the bank.
//!
//! LP side: deposits mint fungible share units of the game alkane itself at
//! the current `bank / total_shares` price (with a virtual offset against
//! the ERC-4626 first-depositor inflation attack); withdrawals burn them
//! pro-rata. Shares are ordinary transferable alkanes.

use alkanes_runtime::{runtime::AlkaneResponder, storage::StoragePointer};
use alkanes_support::{
    cellpack::Cellpack,
    id::AlkaneId,
    parcel::{AlkaneTransfer, AlkaneTransferParcel},
    response::CallResponse,
};
use anyhow::{anyhow, Result};
use bitcoin::hashes::Hash;
use metashrew_support::index_pointer::KeyValuePointer;
use std::sync::Arc;

use cheekyd_math::{
    block_budget, is_win, max_amount_for_exposure, max_bet, max_net_profit, net_exposure,
    payout_total, roll as roll_entropy, shares_for_deposit, t_max, validate_target,
    withdraw_payout, EDGE_BPS, T_MIN,
};

/// Floor for the immutable `min_bet` init param: keeps payout flooring
/// meaningful at the loosest multiplier (×1.05 needs ≥20 units to beat the
/// stake by ≥1; 1000 gives comfortable margin) and blocks dust spam.
const MIN_BET_FLOOR: u128 = 1_000;

/// Creating a canonical table requires seeding its bank with at least this
/// many units of the token, deposited as the creator's LP position — tables
/// are born alive, and spamming the registry costs real tokens.
const MIN_TABLE_SEED: u128 = 1_000;

// ---------- Storage helpers (mirror fireball-game-runtime) ----------

fn ptr(keyword: &str) -> StoragePointer {
    StoragePointer::from_keyword(keyword)
}

fn read_u128(p: StoragePointer) -> u128 {
    let bytes = p.get();
    if bytes.is_empty() {
        0
    } else {
        let mut buf = [0u8; 16];
        let n = bytes.len().min(16);
        buf[..n].copy_from_slice(&bytes[..n]);
        u128::from_le_bytes(buf)
    }
}

fn write_u128(mut p: StoragePointer, v: u128) {
    p.set(Arc::new(v.to_le_bytes().to_vec()));
}

fn read_alkane_id(p: StoragePointer) -> Result<AlkaneId> {
    let bytes = p.get();
    Ok(bytes.as_ref().clone().try_into()?)
}

fn write_alkane_id(mut p: StoragePointer, id: &AlkaneId) {
    let v: Vec<u8> = id.clone().into();
    p.set(Arc::new(v));
}

/// The immutable bet/bank currency, set once at init (mainnet: 2:490).
fn bet_token() -> Result<AlkaneId> {
    read_alkane_id(ptr("/bet_token"))
}

/// Registry slot for the canonical table of `token`. Lives in the CANONICAL
/// template's storage (factory clones have isolated storage and never read
/// theirs) — `CreateTable` is its only writer, one table per token, forever.
fn table_ptr(token: &AlkaneId) -> StoragePointer {
    ptr("/table/").select(&Vec::<u8>::from(token.clone()))
}

/// Sum every incoming transfer of `id` (defends against a caller splitting
/// one asset across several parcels).
fn incoming_sum(transfers: &[AlkaneTransfer], id: &AlkaneId) -> u128 {
    transfers
        .iter()
        .filter(|t| &t.id == id)
        .fold(0u128, |acc, t| acc.saturating_add(t.value))
}

/// Decode a u128-packed string (LE bytes, zero bytes dropped) — the standard
/// alkanes token-name packing, same as alkanes-std-factory-support.
fn trim(v: u128) -> String {
    String::from_utf8(
        v.to_le_bytes()
            .into_iter()
            .filter(|b| *b != 0)
            .collect::<Vec<u8>>(),
    )
    .unwrap_or_default()
}

/// Encode a string as the u128 packing above, truncating to 16 bytes on a
/// char boundary (init args carry one u128 per string).
fn pack_str(s: &str) -> u128 {
    let mut buf = [0u8; 16];
    let mut n = 0;
    for ch in s.chars() {
        let l = ch.len_utf8();
        if n + l > 16 {
            break;
        }
        ch.encode_utf8(&mut buf[n..]);
        n += l;
    }
    u128::from_le_bytes(buf)
}

// ---------- The trait ----------

pub trait CheekydBase: AlkaneResponder {
    // ---- Init ----

    /// One-time. `min_bet` (≥ 1000), the bet/bank token id, and the share
    /// token's name/symbol (one u128 each, standard LE packing; `0` = the
    /// "CheekyD House"/"CHKD" defaults) — immutable afterwards, like
    /// everything else: the edge is a compile-time 1% and there is no
    /// privileged capability of any kind.
    ///
    /// **Factory-ready** (the PERMAFROST pattern): every parameter is an
    /// init argument, so one deployed WASM serves any number of instances —
    /// clone via `[6, TEMPLATE_TX, 0, min_bet, token_block, token_tx, name,
    /// symbol]` to spin up an independent game (own bank, own shares) for
    /// any token without re-deploying bytecode. Direct deploy runs the same
    /// init via `[3, TX, 0, ...]` (gotcha: deploy auto-runs opcode 0).
    fn init_cheekyd(
        &self,
        min_bet: u128,
        token_block: u128,
        token_tx: u128,
        name: u128,
        symbol: u128,
    ) -> Result<CallResponse> {
        if !ptr("/initialized").get().is_empty() {
            return Err(anyhow!("already initialized"));
        }
        if min_bet < MIN_BET_FLOOR {
            return Err(anyhow!("min_bet must be >= {}", MIN_BET_FLOOR));
        }
        if token_block == 0 && token_tx == 0 {
            return Err(anyhow!("bet token id must be non-zero"));
        }
        let context = self.context()?;
        let token = AlkaneId { block: token_block, tx: token_tx };
        // The bank must never be denominated in the game's own shares.
        if token == context.myself {
            return Err(anyhow!("bet token cannot be the game itself"));
        }
        let mut init_marker = ptr("/initialized");
        init_marker.set(Arc::new(vec![1]));

        write_u128(ptr("/min_bet"), min_bet);
        write_alkane_id(ptr("/bet_token"), &token);
        write_u128(ptr("/bank"), 0);
        write_u128(ptr("/total_shares"), 0);

        // Per-instance wallet identity (clones must be tellable apart).
        let name_s = trim(name);
        let symbol_s = trim(symbol);
        ptr("/name").set(Arc::new(if name_s.is_empty() {
            b"CheekyD House".to_vec()
        } else {
            name_s.into_bytes()
        }));
        ptr("/symbol").set(Arc::new(if symbol_s.is_empty() {
            b"CHKD".to_vec()
        } else {
            symbol_s.into_bytes()
        }));

        // Register this instance as the table for its own token (in LOCAL
        // storage — meaningful on the canonical template, harmless no-op
        // bookkeeping on clones, whose registry nobody reads).
        write_alkane_id(table_ptr(&token), &context.myself);

        Ok(CallResponse::forward(&context.incoming_alkanes))
    }

    // ---- CreateTable (the canonical factory front door) ----

    /// Permissionless: spin up the canonical table for `token` — exactly
    /// once per token, forever. Payable in that token: the caller must
    /// attach at least MIN_TABLE_SEED units, and EVERYTHING attached is
    /// deposited into the new table as the caller's LP position (the minted
    /// shares come back with the response) — tables are born alive, and
    /// spamming the registry costs real tokens.
    ///
    /// The table's wallet identity is read from the token itself via the
    /// standard 99/100 opcodes ("Brick" → "Brick House" / "hBRICK") — no
    /// caller-supplied strings, and a dead id fails right here. Records the
    /// clone in `/table/{token}` and returns its id as `(block, tx)` 2×u128
    /// in the response data.
    ///
    /// Only the CANONICAL template (a direct block-4 deploy) hosts the
    /// registry — calling this on a clone reverts. Raw `[6, TX, 0, ...]`
    /// clones remain possible at the protocol level but are NOT in the
    /// registry: ecosystem frontends resolve tables via `ViewTableOf`, so an
    /// unregistered "wildcat" table simply doesn't exist for them.
    fn create_table(&self, token_block: u128, token_tx: u128) -> Result<CallResponse> {
        let context = self.context()?;
        if context.myself.block != 4 {
            return Err(anyhow!(
                "tables are created on the canonical registry (the block-4 template), not on a clone"
            ));
        }
        let token = AlkaneId { block: token_block, tx: token_tx };
        if token_block == 0 && token_tx == 0 {
            return Err(anyhow!("token id must be non-zero"));
        }
        if token == context.myself {
            return Err(anyhow!("the game itself cannot be a bet token"));
        }
        if !table_ptr(&token).get().is_empty() {
            return Err(anyhow!(
                "a table for {}:{} already exists — one per token, forever",
                token_block,
                token_tx
            ));
        }

        // Anti-spam seed: everything attached becomes the first LP deposit.
        let seed = incoming_sum(&context.incoming_alkanes.0, &token);
        if seed < MIN_TABLE_SEED {
            return Err(anyhow!(
                "creating a table seeds its bank: attach at least {} units of the token (got {})",
                MIN_TABLE_SEED,
                seed
            ));
        }

        // Identity comes from the token itself (99 = name, 100 = symbol —
        // the MANDATORY wallet convention; a token that doesn't answer is
        // invisible in wallets and doesn't deserve a table). A failed
        // sub-call reverts the whole tx in alkanes, so non-conforming ids
        // fail right here by design. Fuel is capped: these are tiny reads.
        const IDENTITY_FUEL: u64 = 500_000;
        let tname = String::from_utf8(
            self.staticcall(
                &Cellpack { target: token.clone(), inputs: vec![99] },
                &AlkaneTransferParcel::default(),
                IDENTITY_FUEL,
            )?
            .data,
        )
        .unwrap_or_default();
        let tsym = String::from_utf8(
            self.staticcall(
                &Cellpack { target: token.clone(), inputs: vec![100] },
                &AlkaneTransferParcel::default(),
                IDENTITY_FUEL,
            )?
            .data,
        )
        .unwrap_or_default();
        let table_name = if tname.is_empty() {
            0 // clone init falls back to "CheekyD House"
        } else {
            pack_str(&format!("{} House", tname))
        };
        let table_sym = if tsym.is_empty() {
            0 // fallback "CHKD"
        } else {
            pack_str(&format!("h{}", tsym))
        };

        // Same min_bet across every canonical table (a product constant of
        // this deployment).
        let min_bet = read_u128(ptr("/min_bet"));

        // Factory-clone ourselves. Capture the sequence BEFORE the call
        // (arbuzino gotcha #6): that is the clone's 2:seq id.
        let clone_seq = self.sequence();
        self.call(
            &Cellpack {
                target: AlkaneId { block: 6, tx: context.myself.tx },
                inputs: vec![0, min_bet, token_block, token_tx, table_name, table_sym],
            },
            &AlkaneTransferParcel::default(),
            self.fuel(),
        )?;
        let table = AlkaneId { block: 2, tx: clone_seq };

        // Seed the newborn bank: deposit the whole attached amount into the
        // clone ON BEHALF OF THE CALLER — the minted share units come back in
        // the sub-call response and are forwarded below.
        let deposit_resp = self.call(
            &Cellpack { target: table.clone(), inputs: vec![2, seed] },
            &AlkaneTransferParcel(vec![AlkaneTransfer { id: token.clone(), value: seed }]),
            self.fuel(),
        )?;

        write_alkane_id(table_ptr(&token), &table);

        // Forward: the creator's freshly minted shares + any non-token
        // incoming they attached by mistake (the token itself was consumed
        // by the seed deposit).
        let mut response = CallResponse::default();
        for t in deposit_resp.alkanes.0.iter() {
            response.alkanes.0.push(t.clone());
        }
        for t in context.incoming_alkanes.0.iter() {
            if t.id != token {
                response.alkanes.0.push(t.clone());
            }
        }
        let mut out = Vec::with_capacity(32);
        out.extend_from_slice(&table.block.to_le_bytes());
        out.extend_from_slice(&table.tx.to_le_bytes());
        response.data = out;
        Ok(response)
    }

    // ---- Roll (the game) ----

    /// Bet `amount` of the bet token that this block's roll lands strictly
    /// under `target` (T ∈ [T_MIN, t_max(edge)]; win probability T/65536).
    /// Settles in this same call: on win the payout
    /// `amount · (1 − edge) · 65536 / T` (stake included) is paid from the
    /// bank onto the caller's output; on lose the whole stake joins the bank
    /// (LP yield — no fee is skimmed).
    ///
    /// **Clamp, don't revert**: if the bet's net exposure exceeds what the
    /// caps allow right now (per-bet half-Kelly, or what's left of this
    /// block's 20% budget), the bet is resized DOWN to fit and the excess
    /// returned as change — the roll always happens. `min_amount` is the
    /// caller's floor: if the clamped size falls below it, revert instead
    /// (full refund). `0` = accept any resize. Hard reverts remain only for:
    /// bad target, `amount < min_bet`, underpayment, or capacity below
    /// `min_bet`/`min_amount`.
    ///
    /// **Mint-and-roll**: `amount = 0` is the "bet everything sent" sentinel —
    /// the stake becomes the WHOLE incoming bet-token amount. This is how a
    /// free-mint plays: a tx chains `[mint] → [Roll(target, 0)]`, and since
    /// the mint's size (emission split between the block's minters) is
    /// unknowable when the tx is composed, the roll takes whatever arrived.
    ///
    /// Response data: `(roll, win, payout, effective_amount)` as 4×u128 LE.
    fn roll(&self, target: u128, amount: u128, min_amount: u128) -> Result<CallResponse> {
        let context = self.context()?;
        // FLASH-ATTACK GUARD (critical, immutable): only a direct top-level
        // transaction may roll. When another alkane wraps the call it could
        // observe this block's roll and REVERT the whole tx on a loss —
        // filtering out losses turns the −edge game strictly +EV and drains
        // the bank. A top-level protostone has caller {0,0}; a contract call
        // sets caller to the calling alkane, so this rejects every wrapper.
        // (Verified against alkanes-rs: top-level ctx inits caller =
        // AlkaneId::default(); Call::change_context sets caller = the caller.)
        // Legit flows are unaffected: mint-and-roll chains two SEPARATE
        // top-level protostones (each caller {0,0}), never a contract call.
        if !(context.caller.block == 0 && context.caller.tx == 0) {
            return Err(anyhow!(
                "Roll must be a direct transaction, not a contract call (flash-attack guard)"
            ));
        }
        let edge = EDGE_BPS;
        validate_target(target, edge)?;

        let token = bet_token()?;
        let context = self.context()?;
        let incoming = incoming_sum(&context.incoming_alkanes.0, &token);
        // 0 = bet everything sent (the mint-and-roll flow above).
        let amount = if amount == 0 { incoming } else { amount };

        let min_bet = read_u128(ptr("/min_bet"));
        if amount < min_bet {
            return Err(anyhow!("bet {} below min_bet {}", amount, min_bet));
        }
        if incoming < amount {
            return Err(anyhow!(
                "bet underpaid: sent {} of the bet token, bet is {}",
                incoming,
                amount
            ));
        }

        // What the caps allow right now: the smaller of the per-bet
        // half-Kelly cap and this block's remaining 20% budget (the budget
        // counts REALIZED payouts — losing bets don't consume it, so the
        // maximum a block's wins can extract stays bounded, which is exactly
        // what bounds a block-stuffing, block-withholding miner).
        let bank = read_u128(ptr("/bank"));
        let cap = max_net_profit(bank, edge);
        let h = self.height() as u128;
        let paid_this_block = if read_u128(ptr("/last_win_height")) == h {
            read_u128(ptr("/paid_this_block"))
        } else {
            0
        };
        let block_left = block_budget(bank).saturating_sub(paid_this_block);
        let allowed = cap.min(block_left);

        // Clamp the bet down to fit `allowed` instead of reverting.
        let exposure_asked = net_exposure(amount, target, edge);
        let effective = if exposure_asked > allowed {
            max_amount_for_exposure(allowed, target, edge)
        } else {
            amount
        };
        if effective < min_bet {
            return Err(anyhow!(
                "bank/block capacity too low right now: clamped bet {} below min_bet {}",
                effective,
                min_bet
            ));
        }
        if effective < min_amount {
            return Err(anyhow!(
                "clamped bet {} below your min_amount {} — retry next block or lower the bet",
                effective,
                min_amount
            ));
        }
        let change = incoming - effective;

        let payout = payout_total(effective, target, edge);
        let exposure = payout.saturating_sub(effective);
        // max_amount_for_exposure guarantees this (tested); defensive only.
        if exposure > allowed {
            return Err(anyhow!("clamp invariant broken: {} > {}", exposure, allowed));
        }

        // The roll: sha256(blockhash ‖ txid) of THIS block and THIS tx —
        // unknown to everyone until the block is mined (ord#4541 recipe).
        let header = self.block_header()?;
        let blockhash = header.block_hash().as_raw_hash().to_byte_array();
        let txid = self.transaction_id()?.as_raw_hash().to_byte_array();
        let r = roll_entropy(&blockhash, &txid);
        let win = is_win(r, target);

        // Bookkeeping.
        let bets_key = ptr("/total_bets");
        write_u128(bets_key.clone(), read_u128(bets_key).saturating_add(1));
        let wag_key = ptr("/total_wagered");
        write_u128(wag_key.clone(), read_u128(wag_key).saturating_add(effective));

        if win {
            // bank + effective ≥ payout because exposure ≤ cap ≤ bank·edge/2.
            let new_bank = bank
                .checked_add(effective)
                .ok_or_else(|| anyhow!("bank overflow"))?
                .checked_sub(payout)
                .ok_or_else(|| anyhow!("bank underflow — invariant broken"))?;
            write_u128(ptr("/bank"), new_bank);
            write_u128(ptr("/last_win_height"), h);
            write_u128(
                ptr("/paid_this_block"),
                paid_this_block.saturating_add(exposure),
            );
            let won_key = ptr("/total_won");
            write_u128(won_key.clone(), read_u128(won_key).saturating_add(payout));
        } else {
            let new_bank = bank
                .checked_add(effective)
                .ok_or_else(|| anyhow!("bank overflow"))?;
            write_u128(ptr("/bank"), new_bank);
        }

        // Response: the payout (on win) folded together with any change, plus
        // every non-bet-token incoming handed back untouched.
        let mut response = CallResponse::default();
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(&(r as u128).to_le_bytes());
        out.extend_from_slice(&(win as u128).to_le_bytes());
        out.extend_from_slice(&(if win { payout } else { 0 }).to_le_bytes());
        out.extend_from_slice(&effective.to_le_bytes());
        response.data = out;

        let mut token_back = change;
        if win {
            token_back = token_back.saturating_add(payout);
        }
        for t in context.incoming_alkanes.0.iter() {
            if t.id != token {
                response.alkanes.0.push(t.clone());
            }
        }
        if token_back > 0 {
            response.alkanes.0.push(AlkaneTransfer {
                id: token,
                value: token_back,
            });
        }
        Ok(response)
    }

    // ---- Deposit (LP in) ----

    /// Add exactly `amount` of the bet token to the bank and mint share
    /// units of this contract at the current share price
    /// (`amount · (S + 10^6) / (B + 1)`, virtual-offset — see cheekyd-math).
    /// Overpay above `amount` returned as change; stray other alkanes handed
    /// back. Reverts if the deposit would mint zero shares.
    fn deposit(&self, amount: u128) -> Result<CallResponse> {
        if amount == 0 {
            return Err(anyhow!("deposit must be > 0"));
        }
        let token = bet_token()?;
        let context = self.context()?;
        let incoming = incoming_sum(&context.incoming_alkanes.0, &token);
        if incoming < amount {
            return Err(anyhow!(
                "deposit underpaid: sent {} of the bet token, asked to deposit {}",
                incoming,
                amount
            ));
        }
        let change = incoming - amount;

        let bank = read_u128(ptr("/bank"));
        let total_shares = read_u128(ptr("/total_shares"));
        let shares = shares_for_deposit(amount, bank, total_shares);
        if shares == 0 {
            return Err(anyhow!("deposit too small at the current share price"));
        }

        write_u128(
            ptr("/total_shares"),
            total_shares
                .checked_add(shares)
                .ok_or_else(|| anyhow!("share supply overflow"))?,
        );
        write_u128(
            ptr("/bank"),
            bank.checked_add(amount)
                .ok_or_else(|| anyhow!("bank overflow"))?,
        );

        let mut response = CallResponse::default();
        response.alkanes.0.push(AlkaneTransfer {
            id: context.myself.clone(),
            value: shares,
        });
        if change > 0 {
            response.alkanes.0.push(AlkaneTransfer {
                id: token,
                value: change,
            });
        }
        for t in context.incoming_alkanes.0.iter() {
            if t.id != token {
                response.alkanes.0.push(t.clone());
            }
        }
        Ok(response)
    }

    // ---- Withdraw (LP out) ----

    /// Burn exactly `shares` for their pro-rata slice of the bank
    /// (`shares · (B + 1) / (S + 10^6)` of the bet token). Anytime out —
    /// instant settlement means there are never pending liabilities to wait
    /// on. Overpaid shares returned as change; stray alkanes (incl. the bet
    /// token sent by mistake) handed back alongside the payout. `min_payout`
    /// is a slippage floor: the share price moves with every bet between
    /// broadcast and confirmation, so the call reverts if the payout falls
    /// below it (`0` disables).
    fn withdraw(&self, shares: u128, min_payout: u128) -> Result<CallResponse> {
        if shares == 0 {
            return Err(anyhow!("withdraw amount must be > 0"));
        }
        let token = bet_token()?;
        let context = self.context()?;
        let incoming = incoming_sum(&context.incoming_alkanes.0, &context.myself);
        if incoming < shares {
            return Err(anyhow!(
                "withdraw underpaid: sent {} shares, asked to burn {}",
                incoming,
                shares
            ));
        }
        let change = incoming - shares;

        let total_shares = read_u128(ptr("/total_shares"));
        if shares > total_shares {
            return Err(anyhow!(
                "burn {} exceeds share supply {}",
                shares,
                total_shares
            ));
        }
        let bank = read_u128(ptr("/bank"));
        let payout = withdraw_payout(shares, bank, total_shares);
        if payout == 0 {
            return Err(anyhow!("payout would be zero — not burning shares for nothing"));
        }
        if payout < min_payout {
            return Err(anyhow!(
                "withdraw slippage: payout {} below min_payout {}",
                payout,
                min_payout
            ));
        }

        write_u128(ptr("/total_shares"), total_shares - shares);
        // payout ≤ bank proven in cheekyd-math (withdraw_never_exceeds_bank).
        write_u128(ptr("/bank"), bank.saturating_sub(payout));

        let mut response = CallResponse::default();
        let mut token_back = payout;
        for t in context.incoming_alkanes.0.iter() {
            if t.id == context.myself {
                continue; // consumed (burned) + change pushed below
            }
            if t.id == token {
                token_back = token_back.saturating_add(t.value);
            } else {
                response.alkanes.0.push(t.clone());
            }
        }
        if token_back > 0 {
            response.alkanes.0.push(AlkaneTransfer {
                id: token,
                value: token_back,
            });
        }
        if change > 0 {
            response.alkanes.0.push(AlkaneTransfer {
                id: context.myself.clone(),
                value: change,
            });
        }
        Ok(response)
    }

    // ---- Donate ----

    /// Add exactly `amount` of the bet token to the bank WITHOUT minting
    /// shares — a gift to LPs (seeding, marketing pumps). Overpay returned
    /// as change; other alkanes handed back.
    fn donate(&self, amount: u128) -> Result<CallResponse> {
        if amount == 0 {
            return Err(anyhow!("donation must be > 0"));
        }
        let token = bet_token()?;
        let context = self.context()?;
        let incoming = incoming_sum(&context.incoming_alkanes.0, &token);
        if incoming < amount {
            return Err(anyhow!(
                "donation underpaid: sent {} of the bet token, asked to donate {}",
                incoming,
                amount
            ));
        }
        let change = incoming - amount;

        let bank = read_u128(ptr("/bank"));
        write_u128(
            ptr("/bank"),
            bank.checked_add(amount)
                .ok_or_else(|| anyhow!("bank overflow"))?,
        );

        let mut response = CallResponse::default();
        if change > 0 {
            response.alkanes.0.push(AlkaneTransfer {
                id: token,
                value: change,
            });
        }
        for t in context.incoming_alkanes.0.iter() {
            if t.id != token {
                response.alkanes.0.push(t.clone());
            }
        }
        Ok(response)
    }

    // ---- Standard fungible-token metadata (wallet convention) ----

    /// Wallet/`protorunesbyaddress` probes opcode 99 for the token name.
    /// MUST return a UTF-8 string (arbuzino gotcha #1) — the shares live in
    /// wallets, so this is load-bearing. Per-instance (init arg), so factory
    /// clones are tellable apart in a wallet.
    fn get_name(&self) -> Result<CallResponse> {
        let mut response = CallResponse::default();
        response.data = ptr("/name").get().as_ref().clone();
        Ok(response)
    }

    /// Wallet probes opcode 100 for the token symbol. MUST be UTF-8.
    fn get_symbol(&self) -> Result<CallResponse> {
        let mut response = CallResponse::default();
        response.data = ptr("/symbol").get().as_ref().clone();
        Ok(response)
    }

    // ---- Views ----

    /// `(bank, total_shares)` as 2×u128 LE (32 bytes). Share price is
    /// `(bank + 1) / (total_shares + 10^6)` (virtual offset).
    fn view_bank(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        let mut out = Vec::with_capacity(32);
        out.extend_from_slice(&read_u128(ptr("/bank")).to_le_bytes());
        out.extend_from_slice(&read_u128(ptr("/total_shares")).to_le_bytes());
        response.data = out;
        Ok(response)
    }

    /// `(total_bets, total_wagered, total_won)` as 3×u128 LE (48 bytes).
    fn view_stats(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        let mut out = Vec::with_capacity(48);
        out.extend_from_slice(&read_u128(ptr("/total_bets")).to_le_bytes());
        out.extend_from_slice(&read_u128(ptr("/total_wagered")).to_le_bytes());
        out.extend_from_slice(&read_u128(ptr("/total_won")).to_le_bytes());
        response.data = out;
        Ok(response)
    }

    /// Immutable config: `(min_bet, edge_bps, t_min, t_max, token_block,
    /// token_tx)` as 6×u128 LE (96 bytes).
    fn view_config(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        let token = bet_token().unwrap_or(AlkaneId { block: 0, tx: 0 });
        let mut out = Vec::with_capacity(96);
        out.extend_from_slice(&read_u128(ptr("/min_bet")).to_le_bytes());
        out.extend_from_slice(&EDGE_BPS.to_le_bytes());
        out.extend_from_slice(&T_MIN.to_le_bytes());
        out.extend_from_slice(&t_max(EDGE_BPS).to_le_bytes());
        out.extend_from_slice(&token.block.to_le_bytes());
        out.extend_from_slice(&token.tx.to_le_bytes());
        response.data = out;
        Ok(response)
    }

    /// Registry lookup: the canonical table for `(token_block, token_tx)`
    /// as `(block, tx)` 2×u128 LE — zeros if no table exists yet. Meaningful
    /// on the canonical template (the registry host).
    fn view_table_of(&self, token_block: u128, token_tx: u128) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        let token = AlkaneId { block: token_block, tx: token_tx };
        let id = read_alkane_id(table_ptr(&token)).unwrap_or(AlkaneId { block: 0, tx: 0 });
        let mut out = Vec::with_capacity(32);
        out.extend_from_slice(&id.block.to_le_bytes());
        out.extend_from_slice(&id.tx.to_le_bytes());
        response.data = out;
        Ok(response)
    }

    /// `(max_bet, max_net_profit)` for `target` at the current bank and
    /// edge, as 2×u128 LE (32 bytes) — the frontend's bet-slider limits.
    fn view_max_bet(&self, target: u128) -> Result<CallResponse> {
        let edge = EDGE_BPS;
        validate_target(target, edge)?;
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        let bank = read_u128(ptr("/bank"));
        let mut out = Vec::with_capacity(32);
        out.extend_from_slice(&max_bet(bank, target, edge).to_le_bytes());
        out.extend_from_slice(&max_net_profit(bank, edge).to_le_bytes());
        response.data = out;
        Ok(response)
    }
}

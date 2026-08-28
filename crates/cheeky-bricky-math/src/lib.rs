//! Pure math for **Cheeky Bricky** — an instant roll-under dice game on ALKANES with
//! an LP-owned bankroll and a forever-fixed 1% edge.
//!
//! Mental model:
//!   * A 16-bit `roll` is derived from `sha256(blockhash ‖ txid)` of the block
//!     that mines the bet (the exact algorithm runes canonized in ord#4541).
//!   * The player picks a `target` T; they win iff `roll < T`, so their win
//!     probability is `T / 65536` — the player chooses their own variance, the
//!     house edge is constant across the slider.
//!   * On win the payout (stake included) is `bet · (1 − edge) · 65536 / T`.
//!   * The bankroll (`bank`) is LP-owned: deposits mint fungible shares at the
//!     current `bank / total_shares` price, withdrawals burn them pro-rata.
//!
//! ## The edge is a compile-time constant
//!
//! [`EDGE_BPS`] = 1%, fixed forever — no admin, no auth token, no knob of
//! any kind. The formulas below still take `edge_bps` as a parameter (they
//! are pure and property-tested across hypothetical edges); the contract
//! always passes the constant. The WHOLE edge accrues to the bank: no
//! protocol fee, no external sink — only LPs earn.
//!
//! ## The Kelly cap (the LP guarantee)
//!
//! Maximizing the bankroll's expected log-growth over the bet size lands at
//! **max player net profit = edge · bank, independent of the chosen odds** —
//! one number covers every multiplier. We run **half-Kelly**
//! (`cap = edge/2 · bank`): ~75% of the optimal growth rate at half the
//! variance. Consequences, proven in the tests below:
//!   * per-bet loss ≤ cap → the bank shrinks only multiplicatively, **never
//!     to zero** (an immutable contract that cannot brick);
//!   * expected log-growth is positive → the bank grows almost surely as
//!     volume accumulates (the *sign* of long-run LP return is guaranteed;
//!     the *rate* depends on betting volume).
//!
//! A separate, looser cap bounds the **aggregate net payout per block**
//! ([`BLOCK_BUDGET_BPS`] = 20% of bank): ruin-impossibility only needs the
//! per-event loss < 100%, and the block budget's job is bounding what a
//! block-stuffing miner can extract by withholding — not growth-optimality.
//! The contract CLAMPS oversized bets down to fit the caps instead of
//! reverting (see `max_amount_for_exposure`).
//!
//! ## The probability ceiling `t_max`
//!
//! A win must still beat the stake: the payout multiplier at the loosest
//! allowed target is ≥ ×1.01 ([`MIN_WIN_MULT_BPS`]). At the 1% edge that
//! puts the ceiling at ≈98% win chance (T = 64 238) — the classic dice
//! slider range.
//!
//! ## Share math (virtual offset)
//!
//! Shares are a price ratio against the bank, so the classic ERC-4626
//! first-depositor inflation attack applies. Defense: OpenZeppelin-style
//! virtual offset ([`VIRTUAL_SHARES`]/[`VIRTUAL_BANK`]) — the attack becomes
//! a guaranteed loss for the attacker by a factor of ~10^6 (see
//! `inflation_attack_is_unprofitable`) — plus the contract reverts a deposit
//! that would mint zero shares.

use anyhow::{anyhow, Result};
use bitcoin::hashes::{sha256, Hash, HashEngine};

/// The roll space: rolls are uniform in `[0, 65536)`.
pub const ROLL_SPACE: u128 = 65_536;

/// Basis-point denominator.
pub const BPS: u128 = 10_000;

/// The edge: 1% — the industry-standard dice number (just-dice's), **fixed
/// forever at compile time**. ALL of it accrues to the bank — only LPs earn.
/// (Earlier designs had an auth-token knob walking down a richer launch
/// edge; removed 2026-08-27 on the owner's call — no privileged capability
/// of any kind, the game ships at the minimal standard edge from day one.)
pub const EDGE_BPS: u128 = 100;

/// Aggregate realized net payouts per block ≤ `bank · BLOCK_BUDGET_BPS/BPS`
/// (20%). Miner block-stuffing defense; deliberately MUCH looser than the
/// per-bet Kelly cap (40 max wins deep) so honest
/// multi-winner blocks essentially never hit it — extracting it requires
/// withholding a found block, i.e. forfeiting the whole block reward.
pub const BLOCK_BUDGET_BPS: u128 = 2_000;

/// A win at the loosest target must still pay ≥ ×1.01 of the stake — the
/// classic dice-slider ceiling (satoshidice's 98% end paid exactly this at
/// a ~1% edge). Defines `t_max(edge)`.
pub const MIN_WIN_MULT_BPS: u128 = 10_100;

/// Tightest allowed target: `1` — win chance `1/65536 ≈ 0.0015%`, the maximum
/// multiplier `≈ ×64880` at the 1% edge (the SatoshiDice-style "lessthan 1"
/// longshot ceiling). The bank is NOT exposed by opening this end: the per-bet
/// half-Kelly cap auto-shrinks the stake so `net_exposure ≤ 0.5%` of the bank
/// at ANY multiplier. Extreme targets are simply unplayable until the bank is
/// large enough — the clamped stake would fall below `MIN_BET`, which reverts
/// cleanly (see the game's `roll`). Only `target = 0` (zero win chance) is
/// rejected.
pub const T_MIN: u128 = 1;

/// ERC-4626-style virtual offset: pricing behaves as if `VIRTUAL_SHARES`
/// shares and `VIRTUAL_BANK` bank units always exist. First real deposit
/// mints `amount · 10^6` shares (shares are 10^6 × bet-token base units).
pub const VIRTUAL_SHARES: u128 = 1_000_000;
pub const VIRTUAL_BANK: u128 = 1;

/// Payout numerator for a given edge: `(BPS − edge) · 65536`. Payout (stake
/// included) is `bet · payout_num(edge) / (BPS · T)`.
pub const fn payout_num(edge_bps: u128) -> u128 {
    (BPS - edge_bps) * ROLL_SPACE
}

/// The loosest allowed target for a given edge — the largest T whose payout
/// multiplier is still ≥ [`MIN_WIN_MULT_BPS`]/BPS. Rises as the edge drops.
pub const fn t_max(edge_bps: u128) -> u128 {
    payout_num(edge_bps) / MIN_WIN_MULT_BPS
}

// Compile-time coherence.
const _: () = assert!(EDGE_BPS >= 1 && EDGE_BPS < BPS / 2);
const _: () = assert!(BLOCK_BUDGET_BPS < BPS);
const _: () = assert!(MIN_WIN_MULT_BPS > BPS);
const _: () = assert!(T_MIN < t_max(EDGE_BPS)); // target range non-empty
const _: () = assert!(t_max(EDGE_BPS) < ROLL_SPACE); // ceiling never saturates

// ---------- Entropy ----------

/// The 16-bit roll for a bet mined in the block with hash `blockhash` by the
/// transaction `txid` (both as the 32 raw little-endian hash bytes, i.e.
/// `Hash::to_byte_array` order — NOT the reversed display hex).
///
/// Byte-for-byte the algorithm Casey merged into runes for multi-pointer
/// selection (ordinals/ord#4541): `sha256(blockhash ‖ txid)`, first 16 bytes
/// as a little-endian u128, reduced mod the outcome space. 65536 divides
/// 2^128 so the reduction is exactly uniform.
pub fn roll(blockhash: &[u8; 32], txid: &[u8; 32]) -> u16 {
    let mut engine = sha256::Hash::engine();
    engine.input(blockhash);
    engine.input(txid);
    let hash = sha256::Hash::from_engine(engine).to_byte_array();
    let seed = u128::from_le_bytes(hash[..16].try_into().expect("16 bytes"));
    (seed % ROLL_SPACE) as u16
}

/// A bet on `target` wins iff the roll is strictly under it.
pub fn is_win(roll: u16, target: u128) -> bool {
    (roll as u128) < target
}

// ---------- Bet math (all parameterized by the CURRENT edge) ----------

/// Reject targets outside `[T_MIN, t_max(edge)]`.
pub fn validate_target(target: u128, edge_bps: u128) -> Result<()> {
    let ceiling = t_max(edge_bps);
    if target < T_MIN || target > ceiling {
        return Err(anyhow!(
            "target {} out of bounds [{}, {}] at edge {} bps",
            target,
            T_MIN,
            ceiling,
            edge_bps
        ));
    }
    Ok(())
}

/// Total paid to a winner (stake included):
/// `floor(amount · payout_num(edge) / (BPS · target))`. Floor always favors
/// the bank.
pub fn payout_total(amount: u128, target: u128, edge_bps: u128) -> u128 {
    mul_div(amount, payout_num(edge_bps), BPS * target)
}

/// What the bank risks losing on a win: payout minus the stake (the whole
/// bet reaches the bank — no fee carve-out).
pub fn net_exposure(amount: u128, target: u128, edge_bps: u128) -> u128 {
    payout_total(amount, target, edge_bps).saturating_sub(amount)
}

/// The half-Kelly per-bet cap: max allowed net player profit =
/// `floor(bank · edge / (2 · BPS))`.
pub fn max_net_profit(bank: u128, edge_bps: u128) -> u128 {
    mul_div(bank, edge_bps, 2 * BPS)
}

/// The per-block aggregate budget: `floor(bank · BLOCK_BUDGET_BPS / BPS)`.
pub fn block_budget(bank: u128) -> u128 {
    mul_div(bank, BLOCK_BUDGET_BPS, BPS)
}

/// The largest bet at `target` whose [`net_exposure`] fits `allowed` — the
/// clamp primitive: an oversized bet is resized down to this instead of
/// reverting. Closed form:
/// `amount = allowed · BPS · T / (payout_num(edge) − BPS · T)`; the
/// denominator is > 0 for every valid target (multiplier stays above 1).
pub fn max_amount_for_exposure(allowed: u128, target: u128, edge_bps: u128) -> u128 {
    let denom = payout_num(edge_bps) - BPS * target;
    mul_div(allowed, BPS * target, denom)
}

/// UI helper: the largest bet at `target` under the per-bet Kelly cap.
pub fn max_bet(bank: u128, target: u128, edge_bps: u128) -> u128 {
    max_amount_for_exposure(max_net_profit(bank, edge_bps), target, edge_bps)
}

// ---------- Share math ----------

/// Shares minted for depositing `amount` into `(bank, total_shares)`:
/// `floor(amount · (S + VIRTUAL_SHARES) / (B + VIRTUAL_BANK))`. Bootstrap
/// (empty vault) mints `amount · 10^6`. The contract must reject a zero
/// result (deposit too small at the current share price).
pub fn shares_for_deposit(amount: u128, bank: u128, total_shares: u128) -> u128 {
    mul_div(
        amount,
        total_shares + VIRTUAL_SHARES,
        bank + VIRTUAL_BANK,
    )
}

/// Bet-token paid for burning `shares` against `(bank, total_shares)`:
/// `floor(shares · (B + VIRTUAL_BANK) / (S + VIRTUAL_SHARES))`. Always
/// ≤ `bank` (strictly below `B + 1` since the virtual share never redeems),
/// so the bank cannot be overdrawn — see `withdraw_never_exceeds_bank`.
pub fn withdraw_payout(shares: u128, bank: u128, total_shares: u128) -> u128 {
    mul_div(
        shares,
        bank + VIRTUAL_BANK,
        total_shares + VIRTUAL_SHARES,
    )
}

// ---------- mul_div (256-bit exact) ----------

/// Compute `floor(a * b / d)` exactly via a 256-bit intermediate so `a * b`
/// never overflows `u128`. The true quotient is assumed to fit in `u128` (it
/// does at every call site here). Returns `u128::MAX` on `d == 0` as a
/// panic-free guard — no caller passes `d == 0`.
pub fn mul_div(a: u128, b: u128, d: u128) -> u128 {
    if d == 0 {
        return u128::MAX;
    }
    if let Some(p) = a.checked_mul(b) {
        return p / d;
    }
    let (hi, lo) = mul_u128_wide(a, b);
    div_u256_by_u128(hi, lo, d)
}

/// Full 256-bit product of two `u128`s as `(high, low)` 128-bit limbs.
fn mul_u128_wide(a: u128, b: u128) -> (u128, u128) {
    const LO: u128 = u64::MAX as u128;
    let (a0, a1) = (a & LO, a >> 64);
    let (b0, b1) = (b & LO, b >> 64);

    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;

    let lane0 = p00 & LO;
    let carry1 = p00 >> 64;
    let t1 = carry1 + (p01 & LO) + (p10 & LO);
    let lane1 = t1 & LO;
    let carry2 = t1 >> 64;
    let t2 = carry2 + (p01 >> 64) + (p10 >> 64) + (p11 & LO);
    let lane2 = t2 & LO;
    let carry3 = t2 >> 64;
    let lane3 = carry3 + (p11 >> 64);

    let low = lane0 | (lane1 << 64);
    let high = lane2 | (lane3 << 64);
    (high, low)
}

/// Divide a 256-bit value `(hi, lo)` by `d`, returning the low 128 bits of
/// the quotient. Bit-by-bit long division; the running remainder stays
/// `< d < 2^128` (the shifted-out top bit is tracked explicitly).
fn div_u256_by_u128(hi: u128, lo: u128, d: u128) -> u128 {
    let mut rem: u128 = 0;
    let mut quo: u128 = 0;
    let mut i = 256;
    while i > 0 {
        i -= 1;
        let bit = if i >= 128 {
            (hi >> (i - 128)) & 1
        } else {
            (lo >> i) & 1
        };
        let rem_msb = rem >> 127;
        rem = (rem << 1) | bit;
        if rem_msb == 1 || rem >= d {
            rem = rem.wrapping_sub(d);
            if i < 128 {
                quo |= 1u128 << i;
            }
        }
    }
    quo
}

#[cfg(test)]
mod tests {
    use super::*;

    const E0: u128 = EDGE_BPS; // 100 — the one true edge

    // ── Entropy ─────────────────────────────────────────────────────────────

    /// Pin the exact ord#4541 recipe: sha256(blockhash ‖ txid), first 16
    /// bytes LE as u128, mod the space — recomputed inline as the spec writes
    /// it, independent of the helper's internals.
    #[test]
    fn roll_matches_ord_4541_recipe() {
        let blockhash = [0xABu8; 32];
        let txid = [0x42u8; 32];

        let mut concat = Vec::with_capacity(64);
        concat.extend_from_slice(&blockhash);
        concat.extend_from_slice(&txid);
        let digest = sha256::Hash::hash(&concat).to_byte_array();
        let seed = u128::from_le_bytes(digest[..16].try_into().unwrap());
        let expected = (seed % ROLL_SPACE) as u16;

        assert_eq!(roll(&blockhash, &txid), expected);
    }

    #[test]
    fn roll_is_deterministic_and_txid_sensitive() {
        let bh = [7u8; 32];
        let t1 = [1u8; 32];
        let mut t2 = t1;
        t2[0] = 2;
        assert_eq!(roll(&bh, &t1), roll(&bh, &t1));
        let r1 = roll(&bh, &t1);
        let r2 = roll(&bh, &t2);
        let r3 = roll(&[8u8; 32], &t1);
        assert!(r1 != r2 || r1 != r3, "roll ignores its inputs");
    }

    #[test]
    fn roll_covers_the_space_reasonably() {
        let bh = [3u8; 32];
        let mut seen = std::collections::HashSet::new();
        let (mut low, mut high) = (0u32, 0u32);
        for i in 0..4096u32 {
            let mut txid = [0u8; 32];
            txid[..4].copy_from_slice(&i.to_le_bytes());
            let r = roll(&bh, &txid);
            seen.insert(r);
            if (r as u128) < ROLL_SPACE / 2 {
                low += 1;
            } else {
                high += 1;
            }
        }
        assert!(seen.len() > 3600, "only {} distinct rolls", seen.len());
        assert!(low > 1600 && high > 1600, "skewed halves: {low}/{high}");
    }

    // ── Payout ──────────────────────────────────────────────────────────────

    #[test]
    fn payout_exact_values_at_launch_edge() {
        // Even-money: T = 32768 (p = 1/2) → ×1.98 exactly at the 1% edge.
        assert_eq!(payout_total(1_000_000, 32_768, E0), 1_980_000);
        // Longshot: T = 656 → ×≈98.9.
        assert_eq!(payout_total(1_000_000, 656, E0), 98_903_414);
        // Extreme longshot: T = 1 (= T_MIN, ~0.0015% chance) → ×≈64880 — the
        // SatoshiDice-style ceiling. Stake is Kelly-clamped to dust in practice.
        assert_eq!(payout_total(1_000_000, 1, E0), 64_880_640_000);
        // Grind ceiling: T = t_max(100) = 64_238 → ×≈1.0100 (the 98% end).
        assert_eq!(t_max(E0), 64_238);
        assert_eq!(payout_total(1_000_000, 64_238, E0), 1_010_004);
    }

    #[test]
    fn player_ev_is_minus_edge_for_every_target_and_edge() {
        // p · payout ≤ bet · (1 − edge): exactly the house edge, floors only
        // help the bank. Checked across targets AND reachable edges.
        let amount = 5_000_000u128;
        for edge in [10, E0, 500] {
            for target in [T_MIN, 1_000, 6_554, 32_768, t_max(edge)] {
                let p = payout_total(amount, target, edge);
                assert!(
                    target * p * BPS <= amount * payout_num(edge),
                    "edge {edge} target {target}: payout too generous"
                );
                assert!(
                    target * (p + 1) * BPS > amount * payout_num(edge) - target * BPS,
                    "edge {edge} target {target}: payout too stingy"
                );
            }
        }
    }

    #[test]
    fn win_always_beats_the_stake() {
        for edge in [10, E0, 500] {
            for amount in [1_000u128, 6_900, 100_000, 1_000_000_000] {
                for target in [T_MIN, 32_768, t_max(edge)] {
                    assert!(
                        payout_total(amount, target, edge) > amount,
                        "win pays ≤ stake at edge {edge}, amount {amount}, target {target}"
                    );
                }
            }
        }
    }

    #[test]
    fn validate_target_bounds_follow_the_edge() {
        assert!(validate_target(T_MIN, E0).is_ok());
        assert!(validate_target(t_max(E0), E0).is_ok());
        assert!(validate_target(T_MIN - 1, E0).is_err());
        assert!(validate_target(t_max(E0) + 1, E0).is_err());
        // Property (pure fns): a lower edge would raise the ceiling.
        assert!(t_max(10) > t_max(E0));
        assert!(t_max(E0) > t_max(500));
        // A target above OUR ceiling is rejected at the real edge.
        assert!(validate_target(64_500, 10).is_ok());
        assert!(validate_target(64_500, E0).is_err());
    }

    // ── Kelly cap & clamp ───────────────────────────────────────────────────

    #[test]
    fn kelly_cap_is_half_the_edge() {
        // 0.5% of bank at the 1% edge.
        assert_eq!(max_net_profit(10_000_000_000, E0), 50_000_000);
        // Property: scales with a (hypothetical) edge.
        assert_eq!(max_net_profit(10_000_000_000, 500), 250_000_000);
        assert_eq!(max_net_profit(0, E0), 0);
    }

    #[test]
    fn block_budget_is_twenty_percent() {
        assert_eq!(block_budget(10_000_000_000), 2_000_000_000);
        assert_eq!(block_budget(0), 0);
    }

    #[test]
    fn max_bet_respects_the_cap_and_is_reasonably_tight() {
        for edge in [E0, 500] {
            for bank in [1_000_000u128, 10_000_000_000, 312_500_000_000] {
                let cap = max_net_profit(bank, edge);
                for target in [T_MIN, 6_554, 32_768, t_max(edge)] {
                    let b = max_bet(bank, target, edge);
                    assert!(
                        net_exposure(b, target, edge) <= cap,
                        "edge {edge} bank {bank} target {target}: max_bet busts the cap"
                    );
                    // Tightness only applies when a bet is actually possible.
                    // At an extreme multiplier vs a small bank, max_bet floors
                    // to 0 (target unplayable until the bank grows) — correct,
                    // and there is nothing to be "too small" about.
                    if b > 0 {
                        assert!(
                            net_exposure(b * 2, target, edge) > cap,
                            "edge {edge} bank {bank} target {target}: max_bet far too small"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn clamp_primitive_fits_any_allowance() {
        // max_amount_for_exposure is the clamp: whatever `allowed` remains
        // (per-bet cap or block-budget remnant), the resized bet fits it.
        for edge in [10, E0] {
            for allowed in [0u128, 1, 17, 5_000, 1_000_000, 123_456_789] {
                for target in [T_MIN, 32_768, t_max(edge)] {
                    let a = max_amount_for_exposure(allowed, target, edge);
                    assert!(
                        net_exposure(a, target, edge) <= allowed,
                        "clamp busts allowance: edge {edge} allowed {allowed} target {target}"
                    );
                }
            }
        }
    }

    #[test]
    fn longshots_get_automatically_smaller_limits() {
        let bank = 100_000_000_000u128;
        let even = max_bet(bank, 32_768, E0);
        let ten_x = max_bet(bank, 6_554, E0);
        let long = max_bet(bank, T_MIN, E0);
        let grind = max_bet(bank, t_max(E0), E0);
        assert!(even > ten_x && ten_x > long);
        // The high-probability end allows the LARGEST stakes (bank risks only
        // the thin margin above the stake).
        assert!(grind > even * 10);
    }

    // ── The no-ruin / growth simulation (the LP guarantee, end to end) ─────

    fn lcg(seed: &mut u128) -> u128 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *seed
    }

    /// Apply one max-size bet to the bank; returns the new bank.
    fn apply_bet(bank: u128, target: u128, edge: u128, win: bool) -> u128 {
        let amount = max_bet(bank, target, edge);
        if amount == 0 {
            return bank;
        }
        let payout = payout_total(amount, target, edge);
        assert!(
            payout.saturating_sub(amount) <= max_net_profit(bank, edge),
            "cap violated at bank {bank}, target {target}, edge {edge}"
        );
        if win {
            bank + amount - payout
        } else {
            bank + amount
        }
    }

    #[test]
    fn bank_survives_and_grows_over_random_volume() {
        // With players always betting the cap, the drift is
        // so strongly positive that the u128 bank overflows the simulation —
        // renormalize on the way up and remember that growth happened (the
        // real bank is bounded by the token supply; the contract's
        // checked_add turns the impossible overflow into a revert).
        let mut bank = 100_000_000_000u128;
        let initial = bank;
        let mut renormalized = 0u32;
        let targets = [T_MIN, 6_554, 32_768, t_max(E0)];
        let mut seed = 0xC4EE_C4D0_0000_0001u128;
        for _ in 0..200_000 {
            let target = targets[(lcg(&mut seed) % 4) as usize];
            let r = (lcg(&mut seed) % ROLL_SPACE) as u16;
            bank = apply_bet(bank, target, E0, is_win(r, target));
            assert!(bank > 0, "bank hit zero");
            if bank > initial * 1_000_000 {
                bank = initial;
                renormalized += 1;
            }
        }
        assert!(
            renormalized > 0 || bank > initial,
            "bank shrank over 200k bets: {initial} → {bank}"
        );
    }

    #[test]
    fn bank_survives_a_thousand_consecutive_max_wins() {
        // Worst case: every bet wins at the cap. The bank decays
        // multiplicatively (×(1 − 0.005) per hit) but NEVER reaches zero.
        let mut bank = 100_000_000_000u128;
        for _ in 0..1_000 {
            bank = apply_bet(bank, 32_768, E0, true);
            assert!(bank > 0, "bank hit zero under pure-win adversary");
        }
        assert!(bank < 100_000_000_000u128, "adversary should dent the bank");
    }

    #[test]
    fn growth_still_positive_at_tiny_edges() {
        // Property: the LP guarantee keeps its sign even at a hypothetical
        // 0.1% edge (the fns are pure; the contract only ever uses EDGE_BPS).
        let mut bank = 100_000_000_000u128;
        let initial = bank;
        let mut seed = 0xBEEF_0000_0000_0042u128;
        for _ in 0..200_000 {
            let r = (lcg(&mut seed) % ROLL_SPACE) as u16;
            bank = apply_bet(bank, 32_768, 10, is_win(r, 32_768));
            assert!(bank > 0);
        }
        assert!(bank > initial, "no growth at tiny edge: {initial} → {bank}");
    }

    // ── Share math ──────────────────────────────────────────────────────────

    #[test]
    fn bootstrap_deposit_mints_at_virtual_ratio() {
        assert_eq!(shares_for_deposit(100_000_000, 0, 0), 100_000_000 * 1_000_000);
    }

    #[test]
    fn equal_value_deposits_mint_equal_stakes() {
        let a = shares_for_deposit(1_000_000, 0, 0);
        let b = shares_for_deposit(1_000_000, 1_000_000, a);
        let diff = a.abs_diff(b);
        assert!(diff * 1_000 < a, "unequal stakes: {a} vs {b}");
    }

    #[test]
    fn withdraw_round_trip_never_profits() {
        for (bank, shares_out) in [(0u128, 0u128), (5_000_000, 7_777_777_000_000)] {
            let amount = 3_141_592u128;
            let minted = shares_for_deposit(amount, bank, shares_out);
            let back = withdraw_payout(minted, bank + amount, shares_out + minted);
            assert!(back <= amount, "round trip minted value: {amount} → {back}");
            assert!(amount - back < 10, "round trip loses too much: {amount} → {back}");
        }
    }

    #[test]
    fn withdraw_never_exceeds_bank() {
        for (shares, total, bank) in [
            (u64::MAX as u128, u64::MAX as u128, 1u128),
            (1_000_000_000_000, 1_000_000_000_000, 777),
            (5, 5, 0),
            (u128::MAX / 2, u128::MAX / 2, 123_456_789),
        ] {
            assert!(withdraw_payout(shares, bank, total) <= bank);
        }
    }

    #[test]
    fn inflation_attack_is_unprofitable() {
        let attacker_seed = 1u128;
        let a_shares = shares_for_deposit(attacker_seed, 0, 0); // 10^6
        let mut bank = attacker_seed;
        let mut total = a_shares;

        let donation = 1_000_000_000_000u128; // attacker "donates" 10^12
        bank += donation;

        let victim_in = 100_000_000u128;
        let v_shares = shares_for_deposit(victim_in, bank, total);
        assert!(v_shares > 0, "victim zeroed out — offset too small");
        bank += victim_in;
        total += v_shares;

        let victim_out = withdraw_payout(v_shares, bank, total);
        assert!(
            victim_out * 100 >= victim_in * 99,
            "victim lost too much: {victim_in} → {victim_out}"
        );

        let attacker_out = withdraw_payout(
            a_shares,
            bank - victim_out,
            total - v_shares,
        );
        let spent = attacker_seed + donation;
        assert!(
            attacker_out * 3 < spent * 2,
            "attack too cheap: spent {spent}, can exit with {attacker_out}"
        );
    }

    #[test]
    fn share_solvency_across_random_sequence() {
        let mut bank = 0u128;
        let mut total = 0u128;
        let mut held: Vec<u128> = Vec::new();
        let mut seed = 0xD1CE_0000_0000_0007u128;
        for _ in 0..5_000 {
            match lcg(&mut seed) % 4 {
                0 | 1 => {
                    // Mirror the contract: a mint that would overflow the
                    // share supply reverts (checked_add) instead of panicking
                    // — reachable only in the pathological drained-bank state
                    // where stale shares trade at ~zero.
                    let amount = (lcg(&mut seed) % 1_000_000_000) + 1;
                    let s = shares_for_deposit(amount, bank, total);
                    if s > 0 {
                        if let (Some(nb), Some(nt)) =
                            (bank.checked_add(amount), total.checked_add(s))
                        {
                            bank = nb;
                            total = nt;
                            held.push(s);
                        }
                    }
                }
                2 => {
                    if let Some(s) = held.pop() {
                        let p = withdraw_payout(s, bank, total);
                        assert!(p <= bank, "withdraw {p} > bank {bank}");
                        bank -= p;
                        total -= s;
                    }
                }
                _ => {
                    let win = lcg(&mut seed) % 2 == 0;
                    bank = apply_bet(bank, 32_768, E0, win);
                }
            }
        }
    }

    // ── mul_div primitive ──────────────────────────────────────────────────

    #[test]
    fn mul_div_floors() {
        assert_eq!(mul_div(7, 3, 2), 10);
        assert_eq!(mul_div(10, 10, 3), 33);
    }

    #[test]
    fn mul_div_zero_divisor_returns_max() {
        assert_eq!(mul_div(5, 5, 0), u128::MAX);
    }

    #[test]
    fn mul_div_no_overflow_on_huge_product() {
        assert_eq!(mul_div(u128::MAX, 4, 8), (1u128 << 127) - 1);
    }

    #[test]
    fn mul_div_matches_naive_when_in_range() {
        for &(a, b, d) in &[
            (123u128, 456u128, 7u128),
            (1_000_000, 999_999, 13),
            (0, 99, 5),
            (u64::MAX as u128, u64::MAX as u128, 1_000),
        ] {
            assert_eq!(mul_div(a, b, d), (a * b) / d);
        }
    }
}

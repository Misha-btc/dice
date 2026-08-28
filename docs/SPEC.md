# Cheeky Bricky — instant dice on ALKANES

Status: **implemented** (contract + math + integration tests; regtest e2e
pending). Name: **Cheeky Bricky** (originally "D = DIESEL"; since the bet currency
moved to 2:490 the D is whatever you read into it). Domains cheekybricky.com /
.xyz / .io were free as of 2026-08-25.

Code map: `crates/cheeky-bricky-math` (pure math + the Kelly/no-ruin proofs),
`alkanes/cheeky-bricky-game-runtime` (all logic, trait `CheekyBrickyBase`),
`alkanes/cheeky-bricky-game` (thin `declare_alkane!` wrapper), `src/tests/`
(integration on the alkanes harness — deterministic win/lose forcing by
walking txid candidates).

Sister project of [arbuzino/Fireball](../../arbuzino) — same stack (ALKANES metaprotocol,
immutable contracts, metashrew regtest), opposite temporal design: Fireball is a
multi-block pari-mutuel draw; this is a **single-transaction instant game**.
The bet/bank currency is a single alkane fixed at init and immutable after:
**mainnet 2:490**; regtest/tests use DIESEL 2:0 (the only harness-mintable token).

**Chosen mechanic (2026-08-25): dice vs an LP-owned bankroll.** The design goal,
in the owner's words: minimal-but-nonzero house take, and mechanics that
mathematically guarantee liquidity providers win long-term. Dice + Kelly cap is
the construction that actually delivers that (see the theorem below). Earlier
candidates (coin flip vs pot, progressive 1-of-64 jackpot) are archived in
[ALTERNATIVES.md](ALTERNATIVES.md)-to-be / git history.

## Core idea

One transaction = one bet = one resolution. The player picks their own odds,
sends the bet, and the moment the tx is mined the outcome is settled
**atomically in the same tx**: the payout (if any) lands on the bet tx's own
output 0. No draw phase, no claim, no NFT, no rounds, no keeper — and crucially
**no pending liabilities, ever**, which is what makes open-ended LP
deposit/withdraw trivially safe (online dice sites' hardest accounting problem
does not exist here by construction).

### Entropy (canonical, do not change)

```
hash = sha256(blockhash || txid)        // blockhash = block the tx is MINED in
seed = u128::from_le_bytes(hash[..16])
roll = seed % 65536                     // uniform 16-bit roll
```

This is byte-for-byte the algorithm Casey Rodarmor merged into runes for
multi-pointer selection (ordinals/ord#4541, merged 2026-06-25) — the community-
canonical instant-randomness scheme for Bitcoin. Key properties:

- The **including** block's hash — unknown at broadcast time. (Never use the
  previous block's hash: it is known when composing the tx → free option.)
- `txid` mixed in so every tx in a block gets an independent roll. Grinding txid
  is useless without knowing the future blockhash.
- **One bet per transaction** so the (blockhash, txid) pair is unique per bet.

Runtime support confirmed: `alkanes-runtime` exposes `block_header()` and
`transaction_id()` directly.

## The game: roll-under dice

- `Roll { target, amount, min_amount }`: target T ∈ [T_MIN, t_max(edge)],
  win iff `roll < T`.
  - Win probability `p = T / 65536` — the player picks their own variance.
  - Payout multiplier `m = (10_000 − edge_bps)/10_000 · 65536/T` (total
    returned incl. stake), computed with `mul_div`, floor — rounding always
    favors the bankroll.
  - `T_MIN = 656` (≈1% chance, ×98.9); the ceiling `t_max` is the loosest T
    whose multiplier still beats the stake by ≥ ×1.01
    (`MIN_WIN_MULT_BPS = 10_100`) — at the 1% edge that is **64 238 (≈98%
    chance, ×1.0100)**, the classic dice-slider range.
- **Lose**: the bet joins `/bank`. **Win**: `m·bet` paid from `/bank` to the
  bettor's output 0.

### The edge: 1%, fixed forever

`EDGE_BPS = 100` — a compile-time constant, the industry-standard dice edge
(just-dice's number). No admin, no auth token, no knob: nobody can change it
in either direction, ever. **The whole edge accrues to the bank — players pay
no house rake.** The operator's only cut is a separate 2% fee an LP pays *when
withdrawing* (see "Operator revenue" below) — never a skim on play.

### The Kelly cap — the LP guarantee (this is the whole point)

Maximize the bankroll's expected log-growth over bet size
(`g(x) = (1−p)·ln(1+x) + p·ln(1−x(m−1))`, x = bet/B): the optimum lands at
**max player net profit = edge · B, independent of the chosen odds** — one
number covers every multiplier. We run **half-Kelly** for a smoother path
(~75% of optimal growth at half the variance):

```
cap        = B · edge_bps / (2 · 10_000)     // 0.5% of B at the 1% edge
max_bet(T) = cap / (m − 1)
```

What this buys, stated honestly:

- **No ruin, structurally**: per-bet loss ≤ cap → the bank shrinks only
  multiplicatively, never to zero. The immutable contract cannot brick.
- **Positive log-drift** → B grows almost surely as volume accumulates. This is
  the "mathematical guarantee": the *sign* of long-run LP return is guaranteed
  (verified in tests at the 1% edge and a hypothetical 0.1% one).
- What is NOT guaranteed: the *rate*. Drawdowns happen; LP yield = edge ×
  betting volume, so low traffic = low APY. Don't market it as riskless.

Worked example, bank = $2000 (cap = $10):
×1.98 coinflip → max bet ≈ $10.2; ×9.9 → ≈ $1.12; ×98.9 longshot → ≈ $0.10;
the ×1.01 grind end (98% chance) allows ≈ $1000 (the bank only risks the
thin margin) — the high-probability slider end is the volume machine.

### Clamp, don't revert (+ the per-block budget)

Two caps bound a bet's net exposure: the per-bet half-Kelly cap above, and
the **per-block budget** — aggregate *realized* net payouts per block ≤
`B · 20%` (`BLOCK_BUDGET_BPS = 2000`; losing bets consume nothing). The
budget is the miner defense: a miner stuffing their own block and withholding
on a bad hash extracts at most 20% of B per forfeited block reward — hard and
expensive to ever collect, hence the generous size. It is deliberately much
looser than the per-bet cap (40 max wins deep; ruin-impossibility
only needs per-block loss < 100%; growth-optimality is about the typical
case, and a many-max-winners block is a tail event).

A bet that exceeds what remains is **clamped down to fit** — the excess comes
back as change and the roll still happens — instead of reverting. The caller
opts into all-or-nothing with `min_amount`: if the clamped size falls below
it, revert with a full refund. Hard reverts remain only for bad target, dust
(`amount < min_bet`), underpayment, or capacity below `min_bet`/`min_amount`.

### Two play modes: free roll (mint-and-roll) and a regular bet

`amount > 0` is the regular mode: bet a chosen amount from the wallet.

`amount = 0` is the "bet everything sent" sentinel — the free-roll mode: the
stake becomes the whole incoming bet-token amount. The bet token is a
**free-mint alkane**, and a free-roll transaction chains
`[mint] → [Roll(target, 0, min_amount)]`. The mint's size (the block's
emission split between however many minters showed up) is unknowable when
the transaction is composed, so in this mode the player picks a **number of
mints** (= transactions = rolls), never an amount. Every such roll is free:
the player risks nothing but the network fee; lost mints feed the LP bank.
A mint that overshoots the caps clamps as usual and the excess lands in the
player's wallet. Both modes coexist in the UI as a toggle.

## LP side: the house is a token

- `Deposit { amount }`: bet token in → mint fungible **share units of the game
  alkane itself** (same self-mint machinery as the existing vault; no NFTs).
  Shares minted = `amount · total_shares / B` (1:1 bootstrap).
- `Withdraw { shares, min_payout }`: burn shares → gross `shares · B / total_shares`
  of the bet token; **a 2% exit fee (`EXIT_BPS = 200`) is retained in `/fee_accum`
  and the LP receives 98%** (see "Operator revenue"). The bank is debited by the
  full gross; the fee just moves from `/bank` into `/fee_accum` (both bet-token,
  both held by the contract). `min_payout` is the slippage floor applied to the
  NET amount the LP receives (share price moves with every bet; pattern ported
  from the vault's redeem). Overpaid shares returned as change.
- Share price `B / total_shares` only changes through game results and
  donations — deposits/withdrawals are neutral by construction (`mul_div` from
  `fireball-vault-math`, reused).
- Anytime in, anytime out: with no pending bets there is nothing to lock.
- ⚠ **First-depositor inflation attack** (ERC4626 classic): must be neutralized
  before deploy — virtual shares offset or a burned minimum initial deposit.
  This contract holds withdrawable principal; the audit bar is above Fireball's.
- `Donate { amount }`: grows B without minting — a gift to LPs (marketing
  pumps, treasury seeding).

## Operator revenue: a 2% LP-exit fee (2026-08-27)

Players still pay **no house rake** — the payout multiplier embeds the full 1%
edge and every losing stake lands in the LP bank whole. The operator's only
revenue is a **2% fee on LP withdrawals** (`EXIT_BPS = 200`), chosen by the owner
over a per-bet protocol fee precisely because it never touches the player-facing
odds and only charges capital that is *leaving* the house.

Flow (all in the bet token):
1. `Withdraw` retains 2% of the gross payout in `/fee_accum` (the LP gets 98%).
   The fee stays in the contract's own balance — no external call on the hot path,
   so a broken treasury can never block an LP exit.
2. `SweepFees` (opcode 6, **permissionless**) drains `/fee_accum` into the
   hardcoded `TREASURY_ID = 4:0x7EA5` via its `Collect` op. The destination is
   compiled in, so a permissionless caller cannot redirect it. Checks-effects-
   interactions: `/fee_accum` is zeroed *before* the external call, and alkanes
   reverts all state on a propagated error, so the fee either fully moves or stays
   put — never lost. Reverts on a zero balance.
3. The **treasury** (`cheeky-bricky-treasury`, a standalone contract) holds the
   fees and lets its **owner** withdraw them (`Withdraw{token, amount}`, opcode 2).
   Ownership is a single self-unit key minted to the deployer at init (the
   std-proxy pattern — no external auth factory); the key is spent into the call
   and returned with the payout. `Collect` (opcode 1) is permissionless so the
   game's sweep can always land.

This mirrors arbuzino's fee→vault bridge in *structure* (accrue, then a
permissionless sweep to a hardcoded sink) but the sink is the operator's
collector, not a staker vault.

## Factory (the PERMAFROST pattern)

The contract is **factory-ready**: every parameter — bet token, share
name/symbol — is an init argument (min_bet is a compile-time 1000), nothing instance-specific is
compiled in. One bytecode, a game per token; per-table name/symbol exist so
wallets can tell the share tokens apart.

**The canonical front door is the registry.** The template deployed at
`4:TX` hosts `/table/{token_id}` (clones have isolated storage, so the
registry lives in one place) and registers ITSELF for its own token at
init. `CreateTable{token}` (opcode 4, permissionless) checks "no table for
this token yet", reads the table's identity from the token's own 99/100
("Brick" → "Brick House"/"hBRICK" — no caller-supplied strings; a token
that doesn't answer the mandatory wallet convention reverts by design),
factory-clones the template, records the clone id — **one table per token,
forever** — and returns it. It is NOT payable and NOT seeded: the table is
born EMPTY (bank 0), dormant until someone deposits, exactly like the
template's own table. The creator gets no privilege (edge/min_bet/identity
all fixed or derived), so squatting an empty table is pointless; the
Bitcoin tx fee is the only spam gate. `ViewTableOf{token}` (opcode 105)
resolves a token to its table (zeros = none). Calling CreateTable on a
clone reverts.

Raw `[6, TX, 0, ...]` clones remain possible at the protocol level (that
cannot be forbidden) but are NOT in the registry: ecosystem frontends
resolve tables via `ViewTableOf`, so a "wildcat" table simply doesn't exist
for them. Guards: the bet token must not be the instance itself, and init
runs once per instance (`/initialized`, re-init attack tested). Verified
end-to-end in `factory_test`.

## No privileged capability on the game

The **game** has none — by the owner's final call (2026-08-27). Earlier designs
had an auth token that could lower a richer launch edge over time; it was
removed together with the whole dynamic-edge machinery. The game has no owner,
no admin op, no upgrade path. `SweepFees` is permissionless with a hardcoded
destination, so it grants no privilege. What is deployed is the entire game,
forever.

The **treasury** is a separate contract and *does* have one privileged op —
owner-key `Withdraw`. That privilege is deliberately isolated there: the game
never depends on it, and the treasury holds only already-earned exit fees. The
owner key is minted once at deploy; guard it.

## Storage sketch

```
/initialized          u8
/bet_token            AlkaneId (immutable, init param — mainnet 2:490)
/name, /symbol        bytes  (immutable, init params — per-instance wallet identity)
/table/{token_id}     AlkaneId (registry, template only; CreateTable is the sole writer)
/bank                 u128   (LP bankroll; deposits + losses + donations − wins − withdrawals)
/total_shares         u128   (outstanding share units)
(min_bet is a compile-time constant 1000, not stored)
/last_win_height      u128   (per-block budget bookkeeping)
/paid_this_block      u128   (Σ realized net payouts at last_win_height)
/fee_accum            u128   (accrued 2% LP-exit fees, bet-token; drained by SweepFees)
/total_bets           u128   (stats)
/total_wagered        u128   (stats: Σ effective bets)
/total_won            u128   (stats: Σ payouts)

treasury (separate contract, 4:0x7EA5):
/initialized          u8     (one-shot; init mints 1 owner-key unit to the deployer)
  (no other state — the owner key is a self-unit alkane, fees are the held balance)
```

## Opcode sketch

| Opcode | Name | Args | Notes |
|--------|------|------|-------|
| 0 | `InitCheekyBricky` | `token_block, token_tx, name, symbol` | one-time; name/symbol u128-packed (0 = defaults); min_bet is a compile-time 1000; self-registers in the table registry |
| 4 | `CreateTable` | `token_block, token_tx` | permissionless, NOT payable/seeded (table born empty, dormant until a deposit); identity read from the token's 99/100; ONE canonical table per token, forever; template-only |
| 1 | `Roll` | `target, amount, min_amount` | the game; bet token in `--inputs`; clamped not reverted; `amount = 0` bets everything sent (mint-and-roll); response data `(roll, win, payout, effective_amount)` |
| 2 | `Deposit` | `amount` | mint shares at current price; zero-share deposits revert |
| 3 | `Withdraw` | `shares, min_payout` | burn shares → pro-rata bet token minus the 2% exit fee (LP gets 98%, fee → `/fee_accum`); slippage floor on the net |
| 5 | `Donate` | `amount` | grow the bank, no shares |
| 6 | `SweepFees` | — | permissionless; drain `/fee_accum` → treasury `4:0x7EA5` (hardcoded, CEI, reverts on zero) |
| 99/100 | `GetName`/`GetSymbol` | — | **convention** (share units live in wallets!) — "Cheeky Bricky" / "CBRK" |
| 101 | `ViewBank` | — | `(bank, total_shares)` |
| 102 | `ViewStats` | — | `(total_bets, total_wagered, total_won)` |
| 103 | `ViewConfig` | — | `(min_bet, edge_bps, t_min, t_max, token_block, token_tx)` |
| 104 | `ViewMaxBet` | `target` | `(max_bet, max_net_profit)` at current bank+edge |
| 105 | `ViewTableOf` | `token_block, token_tx` | registry lookup → table `(block, tx)`, zeros if none |

### treasury (4:0x7EA5)

| Opcode | Name | Args | Notes |
|--------|------|------|-------|
| 0 | `Init` | — | one-time; mints 1 owner-key unit to the deployer |
| 1 | `Collect` | — | permissionless; accept + hold any incoming (SweepFees lands here) |
| 2 | `Withdraw` | `token_block, token_tx, amount` | owner-only (attach the 1-unit key, returned with the payout); sends `amount` of the named token to the caller |

## Miner caveat (accepted, immutable)

A miner who finds a block can withhold it if the rolls inside lose them money.
Cost = the full block reward. With the per-block budget, the maximum
extractable advantage per withheld block is bounded by `20% · B`, so the
attack stays irrational while `20% · B < block reward` — document the
resulting soft ceiling on sensible bankroll size rather than pretending it
away. Same argument as ord#4541's spec.

## Flash-attack guard (critical, immutable)

`Roll` accepts a bet ONLY from a direct top-level transaction —
`context.caller == {0, 0}`. Without this, an attacker's wrapper alkane could
call Roll, observe this block's roll in the SAME transaction, and revert on a
loss: filtering out losses turns the −1% game strictly +EV and drains the
bank. This is the classic atomic-revert attack on on-chain randomness.

Why the check is correct (verified against alkanes-rs): a top-level protostone
initializes its context with `caller = AlkaneId::default()` (`{0,0}`);
`Call::change_context` sets `caller` to the calling alkane on any nested call.
So a contract-wrapped Roll always carries a non-zero caller and is rejected;
the only way to reach Roll is a plain transaction, which has no branching and
cannot conditionally revert on the outcome.

Legit flows are unaffected: **mint-and-roll** chains two SEPARATE top-level
protostones (mint, then Roll — each caller `{0,0}`), never a contract call;
`CreateTable` calls `Deposit` (not Roll) on the clone. Positive path proven by every roll test (all top-level); the negative
path is proven by `flash_attack_test` — a test-only `cheeky-bricky-attacker`
alkane forwards a Roll from its own contract context and the call reverts
(no bet recorded, bank untouched, DIESEL refunded).

## Decisions taken at implementation

2026-08-25:
1. **No per-bet protocol fee**: the originally-drafted 0.1% → vault 4:777 bridge
   was REMOVED on the owner's call ("зарабатывать будут только LP"). Superseded
   2026-08-27 by a **2% LP-exit fee to an operator treasury** (see item 8) — the
   player-facing edge is still 100% LP; the operator earns only on withdrawals.
2. **First-depositor mitigation**: OZ-style virtual offset
   (`VIRTUAL_SHARES = 10^6`, `VIRTUAL_BANK = 1`) + revert on zero-share
   deposits. Attack is a guaranteed ~50% loss for the attacker (tested).
3. **Share token branding**: `GetName` = "Cheeky Bricky", `GetSymbol` = "CBRK".
4. **Init params** (immutable): the bet/bank token id — **mainnet 2:490** (a
   parameter, not a hardcode, so the harness can run on mintable DIESEL) plus
   the share name/symbol. `min_bet` is a compile-time constant **1000**
   (2026-08-27, owner's call — dropped as an init arg: 1000 is a safe rounding
   floor, dust is deterred by network fees, and a fixed value removes an
   immutable knob + the mis-scaling footgun).

2026-08-26 (the UX pass — reverts and tiny limits were the owner's concerns):
5. Launch-rich edge + auth-token lowering was designed and built here
   (5% → walkable down to 10 bps), then **superseded on 2026-08-27** — see
   item 9. A 10% per-bet cap was considered and REJECTED with math: the
   zero-log-drift boundary is ~2×edge — 10% at any realistic edge would
   guarantee LP value decay.
6. **Per-block budget 20%** (`BLOCK_BUDGET_BPS = 2000`; owner's call — very
   hard and expensive to ever extract), decoupled from the per-bet
   half-Kelly cap — a money budget of realized net wins, NOT a winner
   count; dozens of ordinary wins fit.
7. **Clamp, don't revert**: oversized bets resize down to fit the caps
   (change returned, roll always happens); optional `min_amount` makes it
   all-or-nothing.
8. Probability ceiling `t_max` = loosest T whose win still beats the stake
   (floor multiplier, see item 9 for the final value).

2026-08-27 (the immutability pass):
9. **Auth token REMOVED, edge fixed at 1% forever** (`EDGE_BPS = 100`, the
   owner: "без auth токена — сразу минимальный процент"). No privileged
   capability on the game remains; the 4:0xffed dependency is gone. The
   floor multiplier was relaxed to **×1.01** (`MIN_WIN_MULT_BPS = 10_100`)
   so the slider reaches the classic **98%** ceiling (T = 64 238) right
   away.

2026-08-27 (operator revenue):
10. **2% LP-exit fee → operator treasury** (`EXIT_BPS = 200`; owner chose
    "operator (treasury)" + "2%" over a per-bet fee). Withdraw keeps 2% of the
    gross in `/fee_accum` (LP nets 98%); the permissionless `SweepFees` (opcode 6)
    pushes it to the hardcoded `TREASURY_ID = 4:0x7EA5`. The treasury is a small
    standalone contract; ownership is a self-unit key (std-proxy pattern), so no
    external auth factory is needed. Kept the fee off the hot path (accrue + sweep,
    not a direct call on withdraw) so a broken treasury can never freeze exits.
    `TREASURY_ID` is hardcoded rather than an init arg — keeps the game's init
    signature and the whole test suite untouched, and prevents redirection.

## Permissionless-table footguns (acknowledged, immutable)

`CreateTable` is permissionless and one-per-token-forever, so a few economic
sharp edges are worth accepting on record BEFORE deploy (none are protocol
bugs; all are isolated to the offending table):

1. **`min_bet` is a fixed compile-time 1000** for every table (no init arg,
   no per-table value). 1000 base units is dust for a normal-decimals token
   (fine), but for a 0-decimals / tiny-supply token it can be a meaningfully
   large minimum, making such a table awkward to bet on. Accepted: flagship
   tokens have ample decimals; exotic low-unit tokens are a niche a fixed
   floor doesn't serve.

2. **A table can be created for another table's SHARES.** Share tokens are
   ordinary alkanes that implement 99/100, so `CreateTable{hBRICK}` succeeds —
   a game where you bet Cheeky Bricky shares. Fully isolated (own bank/shares), just
   exotic. Not blocked because detecting "is this one of my clones' shares"
   generically isn't possible and the nesting is harmless.

3. **Pathological-supply tokens** (> ~3.4·10^32 base units) can overflow the
   bootstrap `seed · 10^6` share mint (the virtual-offset amplifier), wrapping
   that table's share count. Self-inflicted and isolated; realistic tokens are
   far below this. Optional hardening (not applied): make `shares_for_deposit`
   revert on overflow instead of wrapping.

The registry's one-table-per-token rule means a bad/mis-scaled canonical
table for a token is permanent — but anyone can still spin a "wildcat" clone
for their own use; it just won't be the registered one.

## Still open before mainnet

1. Regtest e2e run against a LIVE node (`scripts/e2e-test.sh` is written and
   ready; not yet exercised against docker in this environment). Fuel audit
   (`fuel_audit_test`, CreateTable ≈ 19% of the 3.5M floor) and the
   block-budget / full-withdraw scenarios (`scenario_test`) are DONE.
3. Bank seeding plan (Donate from treasury vs LP-first launch).

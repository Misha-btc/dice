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
  - `T_MIN = 1` (win chance `1/65536 ≈ 0.0015%`, **max multiplier ≈ ×64880** —
    the SatoshiDice-style "lessthan 1" longshot ceiling; only `target = 0` is
    rejected). The ceiling `t_max` is the loosest T whose multiplier still beats
    the stake by ≥ ×1.01 (`MIN_WIN_MULT_BPS = 10_100`) — at the 1% edge that is
    **64 238 (≈98% chance, ×1.0100)**. Opening the longshot end costs the bank
    nothing: the Kelly cap (below) auto-shrinks the stake so `net_exposure` never
    exceeds 0.5% of the bank at ANY multiplier; extreme targets simply floor to a
    dust (or 0) max bet until the bank is large enough, and a clamped bet below
    `MIN_BET` reverts cleanly.
- **Lose**: the bet joins `/bank`. **Win**: `m·bet` paid from `/bank` to the
  bettor's output 0.

### The edge: 1%, fixed forever

`EDGE_BPS = 100` — a compile-time constant, the industry-standard dice edge
(just-dice's number). No admin, no auth token, no knob: nobody can change it
in either direction, ever. **The whole edge accrues to the bank — players pay
no house rake.** The operator's only cut is a fee an LP pays *when withdrawing*,
adjustable within `[0, 2%]` but hard-capped at 2% (see "Operator revenue" below) —
never a skim on play.

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
the ×64880 extreme → max bet ≈ $0.00015 (dust — playable only once the bank is
much larger); the ×1.01 grind end (98% chance) allows ≈ $1000 (the bank only
risks the thin margin) — the high-probability slider end is the volume machine.

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
  of the bet token; **the current exit fee (`/exit_bps`, 0..2%) is retained in
  `/fee_accum` and the LP receives the rest** (see "Operator revenue"). The bank is
  debited by the full gross; the fee just moves from `/bank` into `/fee_accum`
  (both bet-token, both held by the contract). `min_payout` is the slippage floor
  applied to the NET amount the LP receives (share price moves with every bet;
  pattern ported from the vault's redeem). Overpaid shares returned as change.
- Share price `B / total_shares` only changes through game results and
  donations — deposits/withdrawals are neutral by construction (`mul_div` from
  `fireball-vault-math`, reused).
- Anytime in, anytime out: with no pending bets there is nothing to lock.
- ⚠ **First-depositor inflation attack** (ERC4626 classic): must be neutralized
  before deploy — virtual shares offset or a burned minimum initial deposit.
  This contract holds withdrawable principal; the audit bar is above Fireball's.
- `Donate { amount }`: grows B without minting — a gift to LPs (marketing
  pumps, treasury seeding).

## Operator revenue: an LP-exit fee, capped at 2% (2026-08-27)

Players still pay **no house rake** — the payout multiplier embeds the full 1%
edge and every losing stake lands in the LP bank whole. The operator's only
revenue is a fee on LP withdrawals, chosen by the owner over a per-bet protocol
fee precisely because it never touches the player-facing odds and only charges
capital that is *leaving* the house.

The fee is **adjustable by the operator** (`SetExitFee`, opcode 7) but **bounded
by the immutable `EXIT_BPS_MAX = 200` (2%)** — a compile-time ceiling that no
operator action can exceed, so LPs have a permanent guarantee that exiting will
never cost more than 2%. The minimum is 0 (free exits). The live value lives in
`/exit_bps` and defaults to the 2% max at init.

Flow (all in the bet token):
1. `Withdraw` retains the current fee (`/exit_bps`, 0..200 bps) of the gross
   payout in the table's own `/fee_accum` (the LP gets the rest). The fee stays in
   the contract's own balance — no external call, nothing to strand.
2. `WithdrawFees` (opcode 6) pays the whole `/fee_accum` to the caller and zeroes
   it — but ONLY if the caller presents the hardcoded operator auth key
   (`OPERATOR_AUTH_ID = 4:0x7EA5`). The key is spent-and-returned (never
   consumed), so one key drains every table the operator runs. Reverts on a
   missing key or a zero balance.
3. `SetExitFee { bps }` (opcode 7, same auth key) sets `/exit_bps` for that table;
   reverts if `bps > EXIT_BPS_MAX`. One key adjusts every table.

**Why per-table, not a shared treasury.** Each table has exactly one bet token, so
its fee is single-token and stays put until collected — no sink contract holding
funds, no cross-token registry, no external call / CEI concerns, no "drain
everything" enumeration. If the operator runs many tables they simply collect from
each (one auth key works on all). `ViewBank` exposes `/fee_accum` (field 3) so the
operator can see what's owed per table.

**The auth key** is a minimal standalone contract (`cheeky-bricky-operator`) that
holds no funds: `Init` mints one self-unit to the deployer, and that unit *is* the
operator credential (std-proxy pattern — no external auth factory). Whoever holds
it is the operator; lose it and fee access is gone forever (immutable), but LP and
player flows never touch it, so a lost key can never freeze the game. It must be
deployed at the hardcoded slot BEFORE the game (same "deploy your sink first"
requirement any hardcoded-id design has).

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

## Privileged capability: only fee collection

The game has **no admin, no owner, no upgrade path, no edge knob** — an earlier
dynamic-edge auth token was removed with the whole machinery (2026-08-27). The
single privileged action is `WithdrawFees`: the operator collecting the 2% exit
fees a table has *already accrued*, gated by possession of the hardcoded
`OPERATOR_AUTH_ID` key. It cannot change any parameter, cannot touch the bank or
LP shares, and cannot block a withdrawal — it only moves already-earned fee out.
The key is a minimal standalone token minted once at deploy; guard it (a lost key
strands only future fees, never player or LP funds). What is deployed is the
entire game, forever.

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
/exit_bps             u128   (live LP-exit fee 0..200; operator-set via SetExitFee, init=200)
/fee_accum            u128   (accrued LP-exit fees, bet-token; paid out by WithdrawFees)
/total_bets           u128   (stats)
/total_wagered        u128   (stats: Σ effective bets)
/total_won            u128   (stats: Σ payouts)

operator auth (separate contract, 4:0x7EA5):
/initialized          u8     (one-shot; init mints 1 key unit to the deployer)
  (no other state, holds no funds — the key is just a self-unit alkane)
```

## Opcode sketch

| Opcode | Name | Args | Notes |
|--------|------|------|-------|
| 0 | `InitCheekyBricky` | `token_block, token_tx, name, symbol` | one-time; name/symbol u128-packed (0 = defaults); min_bet is a compile-time 1000; self-registers in the table registry |
| 4 | `CreateTable` | `token_block, token_tx` | permissionless, NOT payable/seeded (table born empty, dormant until a deposit); identity read from the token's 99/100; ONE canonical table per token, forever; template-only |
| 1 | `Roll` | `target, amount, min_amount` | the game; bet token in `--inputs`; clamped not reverted; `amount = 0` bets everything sent (mint-and-roll); response data `(roll, win, payout, effective_amount)` |
| 2 | `Deposit` | `amount` | mint shares at current price; zero-share deposits revert |
| 3 | `Withdraw` | `shares, min_payout` | burn shares → pro-rata bet token minus the current exit fee (0..2%, fee → `/fee_accum`); slippage floor on the net |
| 5 | `Donate` | `amount` | grow the bank, no shares |
| 6 | `WithdrawFees` | — | operator-only (attach `OPERATOR_AUTH_ID` key, returned); pays this table's `/fee_accum` to the caller, zeroes it; reverts on a missing key or zero balance |
| 7 | `SetExitFee` | `bps` | operator-only (auth key, returned); set `/exit_bps = bps`; reverts if `bps > 200` (2% ceiling immutable); `0` = free exits |
| 99/100 | `GetName`/`GetSymbol` | — | **convention** (share units live in wallets!) — "Cheeky Bricky" / "CBRK" |
| 101 | `ViewBank` | — | `(bank, total_shares, fee_accum, exit_bps)` — field 3 = accrued fee, field 4 = live fee bps |
| 102 | `ViewStats` | — | `(total_bets, total_wagered, total_won)` |
| 103 | `ViewConfig` | — | `(min_bet, edge_bps, t_min, t_max, token_block, token_tx, exit_bps_max)` — field 7 = 2% ceiling |
| 104 | `ViewMaxBet` | `target` | `(max_bet, max_net_profit)` at current bank+edge |
| 105 | `ViewTableOf` | `token_block, token_tx` | registry lookup → table `(block, tx)`, zeros if none |

### operator auth key (4:0x7EA5)

| Opcode | Name | Args | Notes |
|--------|------|------|-------|
| 0 | `Init` | — | one-time; mints 1 auth-key unit to the deployer |
| 99/100 | `GetName`/`GetSymbol` | — | "Cheeky Bricky Operator" / "CBOP" (wallet convention) |

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
   2026-08-27 by a **2% LP-exit fee to the operator** (see item 10) — the
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
10. **2% LP-exit fee → operator, collected per-table** (`EXIT_BPS = 200`; owner
    chose "operator" + "2%" over a per-bet fee). Withdraw keeps 2% of the gross in
    the table's own `/fee_accum` (LP nets 98%); the operator pulls it with
    `WithdrawFees` (opcode 6), gated by the hardcoded `OPERATOR_AUTH_ID = 4:0x7EA5`
    key. The FIRST cut of this design used a shared treasury contract that every
    table swept into (`SweepFees` + a multi-token registry + `WithdrawAll`); the
    owner then simplified it (2026-08-27) — one table = one token, so per-table
    collection needs no sink contract, no external call, no registry/cap. The auth
    key is a minimal standalone token (self-unit = credential, std-proxy pattern).
    Hardcoded rather than an init arg — keeps the game's init signature and the
    test suite stable, and there is nothing to redirect (auth is possession, the
    payout always goes to the caller). Kept off the LP-withdraw path so a lost key
    can never freeze exits.
11. **Exit fee made adjustable, cap kept immutable** (owner's call, 2026-08-27:
    "менять комиссию но чтобы максимум всё равно всегда был 2%, минимум 0"). The
    live fee moved into `/exit_bps` (init = 200) with an operator-only `SetExitFee`
    (opcode 7) that reverts above `EXIT_BPS_MAX = 200`; `exit_bps()` clamps on read
    too. So the operator can tune the fee anywhere in `[0, 2%]` per table, but the
    2% ceiling is compiled in and unbreakable — LPs keep a hard "never more than
    2% to exit" guarantee. Exposed: live fee in `ViewBank` field 4, the immutable
    ceiling in `ViewConfig` field 7.

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

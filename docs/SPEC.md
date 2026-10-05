# Dice — instant dice on ALKANES

Status: **live on mainnet since 2026-10-02** — the game (template + registry +
the H2S table) at `4:64206` (block 969539), the operator key at `4:57005`
(block 969541); the full record (txids, wasm sha256, toolchain) is
`deployments/mainnet.env`. Name: **Dice** — renamed from "Cheeky Bricky" on
2026-10-01 (the crates, the wasm and the on-chain names: shares show as
"Dice"/"DICE", the operator key as "Dice Operator"/"DOP"). The mainnet main
table (the template itself) is in FARTANE, 2:69, symbol H2S.

Code map: `crates/dice-math` (pure math + the Kelly/no-ruin proofs),
`alkanes/dice-game-runtime` (all logic, trait `DiceBase`),
`alkanes/dice-game` (thin `declare_alkane!` wrapper),
`alkanes/dice-operator` (the operator auth key for `WithdrawFees`/`SetExitFee`,
`4:0xDEAD`),
`alkanes/dice-attacker` (TEST-ONLY fixture: wraps a Roll in a contract call for
`flash_attack_test`; never deployed), `src/tests/`
(integration on the alkanes harness — deterministic win/lose forcing by
walking txid candidates).

Sister project of arbuzino/Fireball — same stack (ALKANES metaprotocol,
immutable contracts, metashrew regtest), opposite temporal design: Fireball is a
multi-block pari-mutuel draw; this is a **single-transaction instant game**.
The bet/bank currency is a single alkane fixed at init and immutable after:
**mainnet 2:69** (FARTANE / H2S); regtest/tests use DIESEL 2:0 (the only harness-mintable token).

**Chosen mechanic (2026-08-25): dice vs an LP-owned bankroll.** The design goal,
in the owner's words: minimal-but-nonzero house take, and mechanics that
mathematically guarantee liquidity providers win long-term. Dice + Kelly cap is
the construction that actually delivers that (see the theorem below). Earlier
candidates (coin flip vs pot, progressive 1-of-64 jackpot) are archived in
git history.

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
    exceeds 1% of the bank at ANY multiplier; extreme targets simply floor to a
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
number covers every multiplier. We run **full Kelly** — the growth-OPTIMAL
fraction (max drift, higher variance / deeper drawdowns than half-Kelly, still
no-ruin):

```
cap        = B · edge_bps / 10_000           // 1% of B at the 1% edge (full Kelly)
max_bet(T) = cap / (m − 1)
```

Full Kelly is the ceiling of sane bankroll sizing: betting *above* `edge · B`
is overbetting — every step past the Kelly fraction lowers long-run growth, and
beyond about 2× that fraction variance overtakes drift and growth turns
negative. So 1% is the max, not a knob to push further.

What this buys, stated honestly:

- **No ruin, structurally**: per-bet loss ≤ cap → the bank shrinks only
  multiplicatively, never to zero. The immutable contract cannot brick.
- **Positive log-drift** → B grows almost surely as volume accumulates. This is
  the "mathematical guarantee": the *sign* of long-run LP return is guaranteed
  (verified in tests at the 1% edge and a hypothetical 0.1% one).
- What is NOT guaranteed: the *rate*. Drawdowns happen; LP yield = edge ×
  betting volume, so low traffic = low APY. Don't market it as riskless.

Worked example, bank = $2000 (cap = $20 at full Kelly):
×1.98 coinflip → max bet ≈ $20.4; ×9.9 → ≈ $2.25; ×98.9 longshot → ≈ $0.20;
the ×64880 extreme → max bet ≈ $0.0003 (dust — playable only once the bank is
much larger); the ×1.01 grind end (98% chance) allows ≈ $2000 (the bank only
risks the thin margin) — the high-probability slider end is the volume machine.

### Clamp, don't revert (+ the per-block budget)

Two caps bound a bet's net exposure: the per-bet full-Kelly cap above, and
the **per-block budget** — aggregate *realized* net payouts per block ≤
`B · 20%` (`BLOCK_BUDGET_BPS = 2000`; losing bets consume nothing). The
budget is the miner defense: however a miner stuffs their own block, a
published block can take at most about 20% of B. It does not by itself make
withholding irrational — see "Miner caveat" for the bank size at which it
starts to pay. It is deliberately much
looser than the per-bet cap (20 max wins deep at full Kelly; ruin-impossibility
only needs per-block loss < 100%; growth-optimality is about the typical
case, and a many-max-winners block is a tail event).

A bet that exceeds what remains is **clamped down to fit** — the excess comes
back as change and the roll still happens — instead of reverting. The caller
opts into all-or-nothing with `min_amount`: if the clamped size falls below
it, revert with a full refund. Hard reverts remain only for a contract caller
(Roll must come from a top-level transaction, caller `{0,0}` — see
"Flash-attack guard"), a second Roll on the same table in the same transaction
(see "One Roll per transaction"), bad target, dust (`amount < min_bet`),
underpayment, or capacity below `min_bet`/`min_amount`.

**What the budget guarantees (external audit, 2026-09-30).** The budget is
recomputed from the *current* bank on every Roll, and a losing bet grows the
bank, so the gross net-wins paid inside one block can exceed 20% of the bank
it started the block with — but only out of losses that landed in that same
block. The invariant that matters holds exactly: **within one block, Rolls
never take the bank below 80% of its start-of-block value.** The bound is about
what players can take: Deposit and Donate only add to the bank, and an LP
Withdraw takes the LP's own share out (not a payout), so those are outside it
— the proof below is over the Rolls of a block. Proof: every win settles
only if `paid_this_block + exposure ≤ 0.2 · bank_now`, and
`bank_now = B₀ + L − W_prev` (losses so far `L ≥ 0`, earlier wins `W_prev`), so
after the last win `W ≤ 0.2 · (B₀ + L − W_prev)` and the end-of-block bank
`B₀ + L − W ≥ 0.8 · (B₀ + L) + 0.2 · W_prev ≥ 0.8 · B₀`. Two
integration tests chain real Rolls in one block (`scenario_test`): max wins until
the budget runs out end at 0.83 · B₀; wins → a ~bank-sized loss → wins pay out
0.33 · B₀ net (above 20%, funded by the loss) and still end above B₀. Freezing
the budget at the block's first bet would only shrink capacity, so it stays as is.

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
  alkane itself** (the contract mints units of itself; no NFTs). With `B` the
  bank and `S` the share supply, shares minted =
  `floor(amount · (S + 10^6) / (B + 1))` — the virtual offset
  `VIRTUAL_SHARES = 10^6`, `VIRTUAL_BANK = 1` (`dice-math`) — so a first
  deposit into an empty table mints `amount · 10^6`.
- `Withdraw { shares, min_payout }`: burn shares → gross
  `floor(shares · (B + 1) / (S + 10^6))` of the bet token (always ≤ `B`);
  **the current exit fee (`/exit_bps`, 0..2%) is retained in
  `/fee_accum` and the LP receives the rest** (see "Operator revenue"). The bank is
  debited by the full gross; the fee just moves from `/bank` into `/fee_accum`
  (both bet-token, both held by the contract). `min_payout` is the slippage floor
  applied to the NET amount the LP receives (share price moves with every bet).
  Overpaid shares returned as change.
- Share price `(B + 1) / (S + 10^6)` only changes through game results and
  donations — deposits/withdrawals are neutral by construction, up to floor
  rounding that favors the bank (`mul_div`, the 256-bit exact `floor(a·b/d)`
  defined in `dice-math`).
- Anytime in, anytime out: with no pending bets there is nothing to lock.
- **First-depositor inflation attack** (ERC4626 classic): neutralized by the
  virtual offset above plus a revert on zero-share deposits — the attack is a
  guaranteed loss for the attacker (`inflation_attack_is_unprofitable`,
  `equal_value_deposits_mint_equal_stakes`, `withdraw_round_trip_never_profits`).
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
`/exit_bps` and defaults to the 2% max at init. **Every change waits
`EXIT_FEE_DELAY = 1008` blocks (~a week)** before it applies — up or down — so an
LP always sees a new fee a week ahead and can leave at the old one first.

Flow (all in the bet token):
1. `Withdraw` retains the current fee (`/exit_bps`, 0..200 bps) of the gross
   payout in the table's own `/fee_accum` (the LP gets the rest). The fee stays in
   the contract's own balance — no external call, nothing to strand.
2. `WithdrawFees` (opcode 6) pays the whole `/fee_accum` to the caller and zeroes
   it — but ONLY if the caller presents the hardcoded operator auth key
   (`OPERATOR_AUTH_ID = 4:0xDEAD`, any one of its three units). The key is
   spent-and-returned (never consumed), so one key drains every table the
   operator runs. Reverts on a missing key or a zero balance.
3. `SetExitFee { bps }` (opcode 7, same auth key) SCHEDULES that table's fee:
   `/exit_bps_next = bps`, `/exit_bps_at = height + 1008`. `Withdraw` uses the fee
   in force at its own height (`exit_bps_at(h)`: the scheduled one from its
   activation height on). A newer call replaces a still-pending change and
   restarts the countdown — scheduling the current fee is how one is cancelled; a
   change that already matured is first folded into `/exit_bps`. Reverts if
   `bps > EXIT_BPS_MAX`. One key adjusts every table. `ViewExitFee` (opcode 106)
   shows `(fee_now, fee_next, activation_height, delay)`.

**Why per-table, not a shared treasury.** Each table has exactly one bet token, so
its fee is single-token and stays put until collected — no sink contract holding
funds, no cross-token registry, no external call / CEI concerns, no "drain
everything" enumeration. If the operator runs many tables they simply collect from
each (one auth key works on all). `ViewBank` exposes `/fee_accum` (field 3) so the
operator can see what's owed per table.

**The auth key** is a minimal standalone contract (`dice-operator`) that
holds no funds: `Init` mints three self-units to the deployer, and each unit *is*
an operator credential (std-proxy pattern — no external auth factory). Whoever
holds one is the operator. Three, so two spares can live apart: lose all three and
fee access is gone forever (immutable); any one stolen is full operator power (the
fees, and the exit fee within 0..2%). LP and player flows never touch the keys, so
a lost key can never freeze the game. Deploy order does not matter: the game
never calls or resolves the operator contract — it only compares incoming
alkane ids to `4:0xDEAD` — so the slot just has to be free when the operator is
deployed (whoever deploys there holds the keys; `deploy-mainnet.sh` re-checks
the slot right before that broadcast). Mainnet deployed the game first (block
969539) and the operator at block 969541.

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

## Privileged capability: only the exit fee

The game has **no admin, no owner, no upgrade path, no edge knob** — an earlier
dynamic-edge auth token was removed with the whole machinery (2026-08-27). The
only privileged actions are the two operator ops, both gated by possession of
the hardcoded `OPERATOR_AUTH_ID` key: `WithdrawFees` (opcode 6) collects the
exit fees a table has *already accrued* in `/fee_accum`, and `SetExitFee`
(opcode 7) schedules a new LP-exit fee within 0..2% that takes effect only
`EXIT_FEE_DELAY = 1008` blocks later. That is all the operator controls: fee
revenue and the fee's level. Neither op can exceed the immutable 2% ceiling,
skip the delay, change the edge or the caps, touch the bank, LP shares or any
player flow, or block a withdrawal.
The keys are three units of a minimal standalone token minted once at deploy;
guard them (losing all three strands only fee revenue and fixes the exit fee at
its last setting — never player or LP funds). What is deployed is the
entire game, forever.

## Storage sketch

```
/initialized          u8
/bet_token            AlkaneId (immutable, init param — mainnet 2:69)
/name, /symbol        bytes  (immutable, init params — per-instance wallet identity)
/table/{token_id}     AlkaneId (registry, read on the template only; written by CreateTable
                      and by init, which self-registers the instance for its own token)
/bank                 u128   (LP bankroll; deposits + losses + donations − wins − withdrawals)
/total_shares         u128   (outstanding share units)
(min_bet is a compile-time constant 1000, not stored)
/last_win_height      u128   (per-block budget bookkeeping)
/paid_this_block      u128   (Σ realized net payouts at last_win_height)
/exit_bps             u128   (LP-exit fee in force 0..200, init=200)
/exit_bps_next        u128   (fee scheduled by SetExitFee)
/exit_bps_at          u128   (its activation height, schedule height + 1008; 0 = none)
/fee_accum            u128   (accrued LP-exit fees, bet-token; paid out by WithdrawFees)
/last_roll_txid       [u8; 32] (txid of the last Roll on this table; enforces one Roll per tx)
/total_bets           u128   (stats)
/total_wagered        u128   (stats: Σ effective bets)
/total_won            u128   (stats: Σ payouts)

operator auth (separate contract, 4:0xDEAD):
/initialized          u8     (one-shot; init mints 3 key units to the deployer)
  (no other state, holds no funds — the key is just a self-unit alkane)
```

## Opcode sketch

| Opcode | Name | Args | Notes |
|--------|------|------|-------|
| 0 | `InitDice` | `token_block, token_tx, name, symbol` | one-time; name/symbol u128-packed (0 = defaults); min_bet is a compile-time 1000; self-registers in the table registry |
| 4 | `CreateTable` | `token_block, token_tx` | permissionless, NOT payable/seeded (table born empty, dormant until a deposit); identity read from the token's 99/100; ONE canonical table per token, forever; template-only |
| 1 | `Roll` | `target, amount, min_amount` | the game; bet token in `--inputs`; clamped not reverted; `amount = 0` bets everything sent (mint-and-roll); response data `(roll, win, payout, effective_amount)` |
| 2 | `Deposit` | `amount` | mint shares at current price; zero-share deposits revert |
| 3 | `Withdraw` | `shares, min_payout` | burn shares → pro-rata bet token minus the current exit fee (0..2%, fee → `/fee_accum`); slippage floor on the net |
| 5 | `Donate` | `amount` | grow the bank, no shares |
| 6 | `WithdrawFees` | — | operator-only (attach `OPERATOR_AUTH_ID` key, returned); pays this table's `/fee_accum` to the caller, zeroes it; reverts on a missing key or zero balance |
| 7 | `SetExitFee` | `bps` | operator-only (auth key, returned); schedules the fee to become `bps` 1008 blocks later (any change; a newer call replaces a pending one); reverts if `bps > 200` (2% ceiling immutable); `0` = free exits |
| 99/100 | `GetName`/`GetSymbol` | — | **convention** (share units live in wallets!) — "Dice" / "DICE" |
| 101 | `ViewBank` | — | `(bank, total_shares, fee_accum, exit_bps)` — field 3 = accrued fee, field 4 = fee in force (bps) at the view's height |
| 102 | `ViewStats` | — | `(total_bets, total_wagered, total_won)` |
| 103 | `ViewConfig` | — | `(min_bet, edge_bps, t_min, t_max, token_block, token_tx, exit_bps_max)` — field 7 = 2% ceiling |
| 104 | `ViewMaxBet` | `target` | `(max_bet, max_net_profit)` at current bank+edge |
| 105 | `ViewTableOf` | `token_block, token_tx` | registry lookup → table `(block, tx)`, zeros if none |
| 106 | `ViewExitFee` | — | `(fee_now, fee_next, activation_height, delay)`; nothing pending = `fee_next == fee_now`, height 0 |

### operator auth key (4:0xDEAD = 57005)

| Opcode | Name | Args | Notes |
|--------|------|------|-------|
| 0 | `Init` | — | one-time; mints 3 auth-key units (any one authorizes) to the deployer |
| 99/100 | `GetName`/`GetSymbol` | — | "Dice Operator" / "DOP" (wallet convention) |

## Arithmetic: nothing wraps

`mul_div` computes `floor(a·b/d)` through a 256-bit intermediate. Its checked
form `mul_div_checked` returns `None` when the true quotient does not fit u128
(`hi ≥ d`) instead of the low 128 bits (external audit, 2026-09-30: the old
version wrapped). Consequences:

- **Deposit**: `try_shares_for_deposit` is checked end to end (the virtual
  offsets are `checked_add`); a deposit whose share count would pass u128 —
  above ~3.4·10^32 units into an empty table, reachable only with a
  giant-supply token, and `CreateTable` is permissionless — **reverts**
  instead of minting a wrapped count.
- **Roll**: a payout past u128 is **unbounded exposure** (`net_exposure =
  u128::MAX`), so the clamp resizes the bet to the cap and pays it exactly;
  a saturated payout is never settled. At a bank at the u128 ceiling itself
  `bank + bet` overflows and the Roll reverts (checked_add).
- Deposit, Donate and Roll settlement grow the bank and the share supply
  through `checked_add` / `checked_sub` (revert on overflow). Withdraw only
  subtracts values proven not to exceed their minuend: `shares ≤ total_shares`
  is enforced, `fee ≤ payout` (≤ 2%), `payout ≤ bank`
  (`withdraw_never_exceeds_bank`). The fee accumulator and the stats saturate.

## Miner caveat (accepted, immutable)

A miner who finds a block can withhold it if the rolls inside lose them money.
Withholding forfeits that block's reward `R` (3.125 BTC subsidy + fees) — less
for a miner who does not bear the full reward (a pool member paid per share,
say, where the pool absorbs the loss). The per-block budget caps what a
*published* block can take at about `20% · B`, but it does not bound the gain
per *withheld* block: near-certain bets (the ×1.01 end, where a max bet is about
the whole bank) let a miner discard only the rare block in which a bet loses.
A miner who bears the full reward therefore profits from withholding once the
bank is worth more than about `2 · R` (≈ 6.4 BTC at today's subsidy). An earlier
version of this section put the threshold at `20% · B < R`, i.e. `5 · R`, which
was too optimistic.

Above that soft ceiling a withholding miner can take up to the per-block budget
from each block it finds; no block can take the bank below 80% of its
start-of-block value, and the attack stops paying once the bank falls back under
the ceiling. LPs should treat about `2 · R` of bank value as the sensible
bankroll ceiling for a table. The same caveat applies to the ord#4541 scheme.

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
`CreateTable` never calls Roll — the only call it makes on the new table is the
factory clone's init (opcode 0, nothing attached), so the table is born empty.
Positive path proven by every roll test (all top-level); the negative
path is proven by `flash_attack_test` — a test-only `dice-attacker`
alkane forwards a Roll from its own contract context and the call reverts
(no bet recorded, bank untouched, DIESEL refunded).

### One Roll per transaction

Every Roll in a transaction reads the same `sha256(blockhash ‖ txid)`, so N Roll
protostones in one tx are not N independent bets but ONE perfectly correlated
bet of N× the size. The full-Kelly cap is enforced per call, not per roll, so
stacking would leave only the 20% block budget as the limit — and at ≥3 stacked
max bets the bank's expected log-growth already turns negative. The table
therefore stores the txid of its last Roll (`/last_roll_txid`) and a second Roll
from the same transaction reverts with a full refund. Separate transactions,
even in the same block, roll independently (different txid) and are unaffected;
mint-and-roll is a single Roll and is unaffected. Proven by
`roll_test::test_second_roll_in_the_same_tx_reverts` (the first Roll points its
change into a second Roll of the same tx; only one bet settles, the rest is
refunded).

## Decisions taken at implementation

2026-08-25:
1. **No per-bet protocol fee**: the originally-drafted 0.1% → vault 4:777 bridge
   was REMOVED on the owner's call ("only LPs will earn"). Superseded
   2026-08-27 by a **2% LP-exit fee to the operator** (see item 10) — the
   player-facing edge is still 100% LP; the operator earns only on withdrawals.
2. **First-depositor mitigation**: OZ-style virtual offset
   (`VIRTUAL_SHARES = 10^6`, `VIRTUAL_BANK = 1`) + revert on zero-share
   deposits. Attack is a guaranteed ~50% loss for the attacker (tested).
3. **Share token branding**: `GetName` = "Dice", `GetSymbol` = "DICE".
4. **Init params** (immutable): the bet/bank token id — **mainnet 2:69** (a
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
   Kelly cap — a money budget of realized net wins, NOT a winner
   count; dozens of ordinary wins fit.
7. **Clamp, don't revert**: oversized bets resize down to fit the caps
   (change returned, roll always happens); optional `min_amount` makes it
   all-or-nothing.
8. Probability ceiling `t_max` = loosest T whose win still beats the stake
   (floor multiplier, see item 9 for the final value).

2026-08-27 (the immutability pass):
9. **Auth token REMOVED, edge fixed at 1% forever** (`EDGE_BPS = 100`, the
   owner: "no auth token — the minimum fee right away"). No privileged
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
    can never freeze exits. Superseded in part: the fixed 2% became an
    operator-set fee within 0..2% (item 11, timelocked by item 18), and the key
    moved to `4:0xDEAD` (item 15).
11. **Exit fee made adjustable, cap kept immutable** (owner's call, 2026-08-27:
    "let the fee change, but the maximum must always stay 2% and the minimum 0"). The
    live fee moved into `/exit_bps` (init = 200) with an operator-only `SetExitFee`
    (opcode 7) that reverts above `EXIT_BPS_MAX = 200`; `exit_bps()` clamps on read
    too. So the operator can tune the fee anywhere in `[0, 2%]` per table, but the
    2% ceiling is compiled in and unbreakable — LPs keep a hard "never more than
    2% to exit" guarantee. Exposed: live fee in `ViewBank` field 4, the immutable
    ceiling in `ViewConfig` field 7.

2026-08-28 (odds range + bankroll sizing):
12. **T_MIN 656 → 1** (owner's call): the longshot end now reaches the
    SatoshiDice-style **×64880** max multiplier (win chance `1/65536`); only
    `target = 0` is rejected. No new bank risk — the Kelly cap auto-shrinks the
    stake to dust/0 at extreme multipliers (a clamped bet below `MIN_BET` reverts
    cleanly), so the extreme is playable only once the bank is large enough.
13. **Half-Kelly → full Kelly** (owner's call): `max_net_profit` moved from
    `edge/2 · B` (0.5%) to `edge · B` (1%) — the growth-OPTIMAL fraction. Faster
    drift, higher variance / deeper drawdowns, still structurally no-ruin. This is
    the ceiling: `edge · B` is the Kelly optimum, and betting above it (toward the
    ~2×edge zero-drift boundary) would turn long-run growth negative — so 1% is a
    max, not a knob to push. Doubles every max-bet limit; the 20% block budget is
    now 20 max-wins deep (was 40).

2026-10-01 (pre-mainnet):
14. **Renamed to Dice** (owner's call): crates, wasm and scripts
    `cheeky-bricky-*` → `dice-*`; on-chain names "Dice"/"DICE" (shares) and
    "Dice Operator"/"DOP" (key).
15. **Ids fixed** (owner's call): the operator key moves to
    `OPERATOR_AUTH_ID = 4:0xDEAD` (57005, was 0x7EA5) and the game deploys at
    4:0xFACE (64206). Both slots were free on mainnet when chosen;
    `deploy-mainnet.sh` re-checks before spending anything.
16. **Three operator keys** (owner's call): `Init` mints 3 interchangeable units
    instead of 1 — one to use, two spares kept apart, so losing one key no longer
    strands the fees. Accepted trade-off: any single unit is full operator power
    (WithdrawFees, SetExitFee within 0..2%); LP and player funds stay out of reach
    either way.
17. **Main table in FARTANE, 2:69 (symbol H2S)** instead of 2:490 (owner's call):
    the template is initialized with 2:69, so it is the canonical H2S table; its
    shares keep the default "Dice"/"DICE" (an explicit name would have to be an
    init arg — owner preferred the default). Tables for any other token still
    come from `CreateTable`, named "<token> House" / "h<SYMBOL>" automatically.
18. **Exit-fee changes are timelocked** (owner's call): `SetExitFee` no longer
    applies at once — every change, up or down, takes effect
    `EXIT_FEE_DELAY = 1008` blocks (~a week) later (`/exit_bps_next`,
    `/exit_bps_at`; `Withdraw` uses the fee in force at its height). LPs always
    get a week's notice and can exit at the old fee before an increase. A newer
    call replaces a pending one; the 2% ceiling is unchanged. New view
    `ViewExitFee` (opcode 106) shows the countdown.

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
   a game where you bet Dice shares. Fully isolated (own bank/shares), just
   exotic. Not blocked because detecting "is this one of my clones' shares"
   generically isn't possible and the nesting is harmless.

3. **Pathological-supply tokens** (> ~3.4·10^32 base units) would overflow the
   bootstrap `seed · 10^6` share mint (the virtual-offset amplifier). Hardened
   2026-09-30: the mint is checked (`try_shares_for_deposit`), so such a deposit
   reverts instead of wrapping that table's share count. Realistic tokens are far
   below this.

The registry's one-table-per-token rule means a bad/mis-scaled canonical
table for a token is permanent — but anyone can still spin a "wildcat" clone
for their own use; it just won't be the registered one.

## Still open

Mainnet has been live since 2026-10-02 (see the status line). Done before
launch: the fuel audit (`fuel_audit_test`, CreateTable ≈ 19% of the 3.5M
floor), the block-budget / full-withdraw scenarios (`scenario_test`) and a
`deploy-mainnet.sh` rehearsal on regtest (`NETWORK=regtest`, 2026-10-01).

1. Bank seeding plan (Donate from treasury vs LP-first launch): the H2S table,
   like every table, is born empty (bank 0) and stays dormant until its bank
   is funded.
2. Regtest e2e run against a LIVE node (`scripts/e2e-test.sh`: deposit →
   rolls → withdraw → `WithdrawFees`) — written and ready, not yet recorded as
   run. The mainnet deploy was verified through its views and deploy traces;
   the LP and player flows are proven on the alkanes test harness.

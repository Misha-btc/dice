# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

**Cheeky Bricky** is an instant roll-under dice game on Bitcoin via the ALKANES metaprotocol.
The bet/bank currency is a single alkane fixed at init and immutable after —
**mainnet: 2:490**; regtest/tests: DIESEL 2:0 (the only harness-mintable token).
Everything is immutable — the 1% edge is a compile-time constant, there is no
privileged capability of any kind (an auth-token edge-lowering knob existed
2026-08-26 → removed 2026-08-27).
One transaction = one bet = one resolution:
the roll comes from `sha256(blockhash ‖ txid)` of the block that mines the bet
(the algorithm runes canonized in ordinals/ord#4541), and the payout lands on the
bet tx's own output atomically. No rounds, no claims, no NFTs, no pending
liabilities — ever.

The bankroll is **LP-owned**: deposits mint fungible CBRK shares (units of the game
alkane itself) at the current `bank/total_shares` price; withdrawals burn them
pro-rata, anytime. The **full-Kelly cap** (max player net profit = 1% of bank per
bet — the growth-optimal fraction, independent of chosen odds — plus ≤20% of bank
in realized net wins per block) makes the bank mathematically unruinable and gives
LPs a positive long-run log-drift — the "LPs win in the long run" guarantee is the
core design goal.

**Operator revenue = a mutable LP-exit fee, capped at 2%.** Players pay NO house
rake — the whole 1% edge still goes to LPs. The operator earns only when an LP
*withdraws*: the current fee (`/exit_bps`, 0..200 bps) of the gross payout is
retained in the table's own `/fee_accum` (LP gets the rest). The fee is
**operator-adjustable** via `SetExitFee` (opcode 7) but **can never exceed the
compile-time `EXIT_BPS_MAX = 200` (2%)** — a hard, immutable LP guarantee; 0 is
allowed (free exits). Each table holds its own fee in its own single bet token —
no shared treasury, no cross-token registry. The operator collects it per-table
via `WithdrawFees` (opcode 6). Both ops are gated by a **hardcoded auth key**
(`OPERATOR_AUTH_ID = 4:0x7EA5`, a minimal standalone token minted to the deployer);
one key controls every table. No external call on the path, so nothing can strand
the fee.

**Immutable by design** (same philosophy as sister project
[arbuzino/Fireball](../arbuzino)): no proxy, no upgrade path, no admin *on the
game*. Every constant is compiled in. The only privileged action is the operator
collecting fees they already earned, gated by possession of the auth key — losing
the key means losing fee access (immutable), but LP and player flows never touch
it.

Read `docs/SPEC.md` for the full design (including the Kelly derivation and the
miner-withholding analysis).

## Build Commands

```bash
cargo build                                  # native build (IDE/check)
cargo test -p cheeky-bricky-math                   # 25 unit tests (pure math, fast)
./scripts/build-wasms.sh                     # wasm32 build + vendor into src/tests/wasm/
cargo test --lib -- --test-threads=1         # integration tests (alkanes harness)
# --test-threads=1 because tests share global metashrew state

# Regtest cycle (requires docker compose + alkanes-cli, see arbuzino memory):
ALKANES_RS=~/subfrost/alkanes-rs ./scripts/reset-regtest.sh
ALKANES_RS=~/subfrost/alkanes-rs ./scripts/deploy-regtest.sh --build
```

## Repository Layout

```
cheeky-bricky/
├── Cargo.toml                       # workspace + test package (git deps float;
│                                    #  Cargo.lock is seeded from arbuzino's so both
│                                    #  projects build identical alkanes revisions —
│                                    #  do NOT add rev= pins, they split the dep graph)
├── alkanes/
│   ├── cheeky-bricky-game/                # thin declare_alkane! wrapper → cheeky_bricky_game.wasm
│   ├── cheeky-bricky-game-runtime/        # all logic (trait CheekyBrickyBase)
│   ├── cheeky-bricky-operator/            # fee-withdrawal auth key (mints 1 key at init) → cheeky_bricky_operator.wasm
│   └── cheeky-bricky-attacker/            # TEST-ONLY fixture: wraps a Roll (flash-attack test)
├── crates/
│   └── cheeky-bricky-math/                # pure math: roll, payout, Kelly, share pricing
├── src/tests/                       # integration tests (harness copied from arbuzino)
│   ├── helpers.rs / foundation.rs / vendor.rs / test_logger.rs
│   ├── lp_test.rs                   # deposit/withdraw/donate wiring
│   ├── roll_test.rs                 # the game, with DETERMINISTIC win/lose forcing
│   ├── factory_test.rs              # per-token clones (PERMAFROST pattern)
│   ├── flash_attack_test.rs         # Roll rejects contract callers (cheeky-bricky-attacker fixture)
│   ├── fuel_audit_test.rs           # every opcode ≤ 3.5M fuel floor (CreateTable is the heavy one)
│   ├── scenario_test.rs             # same-block budget, full withdraw + revive
│   ├── exit_fee_test.rs             # 2% exit fee → auth-gated WithdrawFees by the operator
│   └── wasm/                        # vendored cheeky_bricky_game.wasm (+ operator, attacker)
├── docs/SPEC.md                     # the design document — read this first
└── scripts/                         # regtest CLI helpers (source scripts/_env.sh)
```

## Key Constants (immutable, `crates/cheeky-bricky-math/src/lib.rs`)

```rust
EDGE_BPS  = 100          // 1%, FIXED FOREVER — ALL of it accrues to LPs (no house
                         // rake on play; the ONLY operator cut is the exit fee below)
// per-bet cap = edge · bank (full Kelly): 1% — growth-optimal, no-ruin; 1% is the max, not a knob
BLOCK_BUDGET_BPS = 2000  // Σ realized net payouts per block ≤ 20% of bank (miner defense)
MIN_WIN_MULT_BPS = 10_100 // win must pay ≥ ×1.01 → t_max = 64_238 (≈98% chance,
                          // the classic dice-slider ceiling)
T_MIN     = 1            // win chance 1/65536 ≈ 0.0015%; MAX multiplier ≈ ×64880
                         // (SatoshiDice-style "lessthan 1" ceiling). Safe because the
                         // full-Kelly cap auto-shrinks the stake at extreme multipliers —
                         // extreme targets just floor to a dust/0 max bet on a small bank
                         // (clamped < MIN_BET → clean revert). Only target 0 is rejected.
VIRTUAL_SHARES = 1_000_000  // ERC-4626 inflation-attack defense (+ zero-share revert)
```

Plus one constant in `cheeky-bricky-game-runtime` (not math):

```rust
EXIT_BPS_MAX = 200         // 2% — IMMUTABLE ceiling on the LP-exit fee (hard LP guarantee)
/exit_bps (storage)        // live fee 0..200, operator-set via SetExitFee; init defaults to 200
OPERATOR_AUTH_ID = 4:0x7EA5 // hardcoded auth key; WithdrawFees + SetExitFee require it (one key, all tables)
```

Oversized bets are **clamped down** to fit the caps (excess returned as change,
the roll still happens); `min_amount` on Roll opts into all-or-nothing.

The Kelly result that makes this work: the growth-optimal cap on player net profit
is `edge · bank` **regardless of the player's chosen odds** (derived in SPEC.md,
enforced in `Roll`, proven in `cheeky-bricky-math` tests incl. a 200k-bet no-ruin sim).

## Opcodes (cheeky-bricky-game, one contract = game + share token)

| Opcode | Name | Args | Notes |
|--------|------|------|-------|
| 0 | `InitCheekyBricky` | `token_block, token_tx, name, symbol` | one-time; name/symbol u128-packed LE (0 = "Cheeky Bricky"/"CBRK"); min_bet is a compile-time 1000 (not an arg); direct deploy `[3,TX,0,…]` or **factory clone `[6,TEMPLATE_TX,0,…]`**; bet token ≠ self |
| 1 | `Roll` | `target, amount, min_amount` | the game; bet token in; CLAMPED not reverted; **`amount = 0` bets everything sent** — the primary mint-and-roll flow (`[mint] → [Roll(target,0)]`, mint size unknowable at compose time); response data `(roll, win, payout, effective_amount)` 4×u128 LE |
| 2 | `Deposit` | `amount` | bet token → mint CBRK shares at current price; zero-share deposits revert |
| 3 | `Withdraw` | `shares, min_payout` | burn shares → pro-rata bet token; **2% exit fee retained in /fee_accum, LP gets 98%**; slippage floor applies to the NET (0=off) |
| 4 | `CreateTable` | `token_block, token_tx` | permissionless registry front door, NOT payable/seeded (table born empty, dormant until a deposit); identity read from the token's own 99/100 ("Brick"→"Brick House"/"hBRICK"); ONE canonical table per token forever (template-only; raw [6,TX] clones possible but unregistered = invisible to frontends); returns the new table id |
| 5 | `Donate` | `amount` | grow bank, mint nothing (gift to LPs) |
| 6 | `WithdrawFees` | — | **operator-only** (attach the `OPERATOR_AUTH_ID` key, spent-and-returned); pays this table's whole `/fee_accum` (bet token) to the caller and zeroes it. Reverts on a missing key or zero balance. No external call |
| 7 | `SetExitFee` | `bps` | **operator-only** (auth key, returned); set `/exit_bps = bps`, reverts if `bps > 200` (the 2% ceiling is immutable). `0` = free exits |
| 99/100 | `GetName`/`GetSymbol` | — | "Cheeky Bricky" / "CBRK" — **wallet convention, load-bearing** |
| 101 | `ViewBank` | — | `(bank, total_shares, fee_accum, exit_bps)` — field 4 is the live fee |
| 102 | `ViewStats` | — | `(total_bets, total_wagered, total_won)` |
| 103 | `ViewConfig` | — | `(min_bet, edge, t_min, t_max, token_block, token_tx, exit_bps_max)` — field 7 is the immutable 2% fee ceiling |
| 104 | `ViewMaxBet` | `target` | `(max_bet, max_net_profit)` at current bank |
| 105 | `ViewTableOf` | `token_block, token_tx` | registry lookup → table `(block, tx)`, zeros if none |

### cheeky-bricky-operator (4:0x7EA5) — fee-withdrawal auth key

Standalone, holds NO funds. It's just the key: `Init` mints one self-unit to the
deployer, and the game's `WithdrawFees` requires possession of it. Implements the
99/100 wallet convention so the key displays cleanly.

| Opcode | Name | Args | Notes |
|--------|------|------|-------|
| 0 | `Init` | — | one-time; mints 1 auth-key unit to the deploy output |
| 99/100 | `GetName`/`GetSymbol` | — | "Cheeky Bricky Operator" / "CBOP" |

## Storage Layout

```
/initialized       u8
/bet_token         AlkaneId (immutable init param — mainnet 2:490)
/name, /symbol     bytes (immutable init params — per-instance wallet identity)
(min_bet: compile-time const 1000, not stored)
/table/{token_id}  AlkaneId (canonical-table registry; template-only, CreateTable-only writer)
/min_bet           u128  (immutable init param)
/bank              u128  (LP bankroll: deposits + donations + losses − wins − withdrawals)
/total_shares      u128  (CBRK supply)
/last_win_height   u128  (per-block budget bookkeeping)
/paid_this_block   u128  (Σ realized net payouts at last_win_height)
/exit_bps          u128  (live LP-exit fee 0..200; operator-set via SetExitFee, init=200)
/fee_accum         u128  (accrued LP-exit fees, bet-token; paid out by WithdrawFees)
/last_roll_txid    [u8; 32] (txid of the last Roll on this table — one Roll per tx)
/total_bets, /total_wagered, /total_won   u128  (stats)
```

## Critical Gotchas

1. **All of arbuzino's gotchas apply** (this repo copies its harness): the 99/100
   wallet convention, deploy auto-runs opcode 0, `--inputs "block:tx:amount"`,
   varint decoding in view.sh, `protorunesbyaddress` regtest flakiness.
2. **Do not pin `rev=` on the git deps.** alkanes-rs internally depends on
   metashrew by floating git ref; a rev pin here creates a SECOND copy of
   `metashrew_support` in the graph and trait-mismatch compile errors. Revisions
   are locked by Cargo.lock (seeded from arbuzino's — keep them in sync).
3. **Entropy byte order**: raw `Hash::to_byte_array()` bytes (internal LE order),
   NOT display hex. `cheeky_bricky_math::roll` must stay byte-for-byte equal to ord#4541
   (`sha256(blockhash ‖ txid)`, first 16 bytes LE u128, mod 65536) — it's pinned by
   `roll_matches_ord_4541_recipe`.
4. **Deterministic outcome forcing in tests**: the harness block header is fixed at
   creation (merkle root not recomputed), so the roll varies ONLY via txid — tests
   walk the routed-DIESEL amount to flip the predicted outcome
   (`roll_test::build_roll_forced`). Every forced test doubles as an
   entropy-equivalence proof between test-side math and the contract.
5. **One Roll per transaction per table — contract-enforced.** Every Roll in a tx
   would read the same `sha256(blockhash ‖ txid)`, so N stacked Rolls are one
   correlated bet of N× the size: the per-bet Kelly cap binds per call, not per
   roll, and only the 20% block budget would be left (≥3 stacked max bets already
   make the bank's log-drift negative). The table stores `/last_roll_txid` and a
   second Roll from the same tx reverts (full refund). Separate txs in the same
   block are unaffected; mint-and-roll is one Roll, so it is unaffected too.
6. **A win can revert on the per-block cap** if earlier wins in the same block
   drained it: the bet refunds in full and can be re-sent next block. This is the
   miner-stuffing defense working as intended, not a bug.
7. **mint_diesel in tests aliases its outpoint** (null-input tx → same txid every
   call): mint at most once per test.

## Security invariants (immutable — do not weaken)

- **Roll is top-level-only**: `context.caller == {0,0}` or revert — the
  flash/atomic-revert guard. Wrapper alkanes can't filter losses.
- **One Roll per transaction per table**: `/last_roll_txid == txid` or revert —
  stacked Rolls in one tx would share one roll and dodge the per-bet Kelly cap.
- **One-shot init** (`/initialized`), **bet token ≠ self**, **one table per
  token** (registry), **the only privileged ops are `WithdrawFees` and
  `SetExitFee`** (both auth-gated; they only touch fee revenue, never the bank or
  LP shares), **rounding favors the bank**, **Kelly + block caps** bound every
  payout.
- **Exit-fee safety**: the fee is capped at the immutable `EXIT_BPS_MAX` (2%) — 
  `SetExitFee` reverts above it and `exit_bps()` clamps defensively, so no operator
  action can ever make exiting cost more than 2%. The fee accrues in the table's
  own `/fee_accum` and `WithdrawFees` pays it from the balance the game already
  holds — no external call, nothing to strand. LP `Withdraw` never touches the auth
  path, so the operator (or a lost key) can never block or freeze an LP exit.

## Test Coverage

- **Math (28, `cargo test -p cheeky-bricky-math`)**: ord#4541 recipe pin, payout
  exactness/EV bounds (property-tested across hypothetical edges), Kelly caps
  + clamp primitive, t_max, 200k-bet no-ruin + growth sims,
  1000-consecutive-wins survival, share pricing (virtual offset, round-trip,
  solvency fuzz, inflation-attack unprofitability), mul_div 256-bit, **no
  wrapping: a quotient past u128 is refused (`mul_div_checked`), an oversized
  deposit into an empty table is refused, an overflowing payout is unbounded
  exposure so the clamp still holds**.
- **Integration (26, `cargo test --lib -- --test-threads=1`)**: deposit/withdraw/
  donate wiring incl. change + slippage floor + revert refunds, **withdraw nets
  98% with the 2% fee held in /fee_accum** (asserted via ViewBank field 3)
  (`lp_test`); exit fee — accrue on withdraw → operator collects via auth-gated
  `WithdrawFees` (keyless attempt reverts; empty-fee reverts and refunds the key);
  **`SetExitFee` lowers the cut (1% nets 99%), 0 = free exit, > 200 reverts,
  keyless reverts** (`exit_fee_test`);
  forced win/lose settlement, **a second Roll in the same tx reverts (one Roll per
  tx, full refund)**, `amount=0` bets-everything sentinel,
  oversized-bet CLAMP + `min_amount` all-or-nothing revert, bounds/dust
  rejections, ViewMaxBet vs math, LP appreciation after losses (`roll_test`);
  factory clone independence + per-clone identity, re-init + table registry (`factory_test`); contract-wrapped Roll rejected while top-level accepted (`flash_attack_test`); per-opcode fuel ≤ 3.5M floor incl. CreateTable ≈ 19% (`fuel_audit_test`); two same-block wins + full-withdraw→dormant→revive (`scenario_test`).

## Relationship to arbuzino

Shares the change-return contract patterns, the test harness, and the deploy
conventions — but is a **standalone repo** with no runtime link to arbuzino
(no fee bridge to vault 4:777). The fee model is deliberately simpler than
arbuzino's accrue-then-sweep bridge: each table keeps its own single-token fee and
the operator pulls it directly with an auth key — no sink contract holding funds,
no external call. **LPs earn the edge, the operator earns the 2% LP-exit fee.**
Bet currency differs too: 2:490 on mainnet vs Fireball's DIESEL.

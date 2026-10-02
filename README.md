# Dice

Instant roll-under dice on Bitcoin via the ALKANES metaprotocol. One transaction
= one bet = one resolution: the outcome is decided by `sha256(blockhash || txid)`
of the block that mines the bet, and the payout lands on the bet tx's own output.
No rounds, no claims, no NFTs. The bankroll is **LP-owned**: the whole **1% edge
(fixed forever)** accrues to LPs — players pay no house rake. The operator's only
revenue is a fee an LP pays when withdrawing — **operator-adjustable, but
hard-capped at 2% forever, and every change takes effect only ~a week (1008
blocks) after it is announced** (`SetExitFee`, opcode 7; 0 = free exits) —
collected per-table with a hardcoded auth key (`WithdrawFees`, opcode 6). Main
table currency: FARTANE, alkane `2:69` (symbol H2S; immutable init param; regtest
uses DIESEL); tables for other tokens come from `CreateTable`.

Sister project of [Fireball / arbuzino](https://arbuzino.com) — shares the
immutable-by-design rules and the tooling, nothing at runtime.

**Status: implemented, pre-mainnet.** Read [docs/SPEC.md](docs/SPEC.md).

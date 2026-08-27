# CheekyD

Instant roll-under dice on Bitcoin via the ALKANES metaprotocol. One transaction
= one bet = one resolution: the outcome is decided by `sha256(blockhash || txid)`
of the block that mines the bet, and the payout lands on the bet tx's own output.
No rounds, no claims, no NFTs — and **the house is 100% LP-owned**: the whole
edge (5% at launch, lowerable-only via the deploy-time auth token) accrues to
the bankroll; there is no protocol fee of any kind. Bet currency: the alkane
`2:490` (immutable init param; regtest uses DIESEL).

Sister project of [Fireball / arbuzino](https://arbuzino.com) — shares the
immutable-by-design rules and the tooling, nothing at runtime.

**Status: implemented, pre-mainnet.** Read [docs/SPEC.md](docs/SPEC.md).

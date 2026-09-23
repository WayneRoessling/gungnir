# A fully populated v3 snapshot round-trips

GAP-112 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
filed by the GAP-067 walk of 2026-09-16
([`../2026-09-16/gap-067-walk.md`](../2026-09-16/gap-067-walk.md)).

## What was wrong

The `gungnir-api` verification row's zero-loss clause was checked on sparse snapshots
only -- `gungnir-api/src/v3/mod.rs`'s own tests, and `party.rs`'s three tracks with no
plan, requirement, bearing ray or queue item. A field a serializer silently dropped from
a real, populated snapshot would have passed every one of them.

## What had changed since the gap was filed

The gap's own action named a v2 `SnapshotResponse` and `GET /v2/snapshot`, and its
evidence pointed at `mod tests` in `gungnir-api/src/v2/mod.rs`. GAP-130 (D-56, D-60,
DN-31 §5.1 amendment 1) retired that interface whole, the day decision, plan and
queue-item identifiers became UUID v7 strings: `/v2` now answers `410 Gone`
(`gungnir-api/tests/retired_v2.rs`), and there is no `v2::SnapshotResponse` left to
round-trip. `verification-capability-table.md`'s `gungnir-api` row still read "v2 snapshot
schema round-trip"; that wording is corrected to "v3" in the same change as this test,
since it names an interface that no longer exists rather than changing what the row
requires.

## What was built

`gungnir-api/tests/full_snapshot_round_trip.rs` builds a `SnapshotResponse` with every
field away from its default: a track with non-identity state and covariance, a
`Hostile` classification, a `Parties` releasability, full provenance and quality; an
`Intercept` plan with a solution; non-default `SystemHealth`; a `Tasked` collection
requirement; a bearing ray; non-zero pipeline stats; and a queued item.

**The HTTP half is hand-rolled over a plain `TcpStream`, not a client crate.** Every
existing file in `gungnir-api/tests/` already makes this choice --
`party.rs`'s own comment says why: pulling in an HTTP client crate for one test would be
a new dependency edge this crate's production code does not have, when a POST and a GET
by hand are a few lines. `bind`/`serve_on` (unlike the TLS-carrying tests in this
directory) are unencrypted loopback, so this file needs no certificate authority either.

## What the tests hold

`a_fully_populated_snapshot_survives_plain_serde` round-trips the snapshot through
`serde_json` directly and asserts full equality, with the fields the gap's own words
call out -- releasability, covariance, classification -- asserted individually so a
failure names which one went missing.

`a_fully_populated_snapshot_survives_an_operators_get` publishes the same snapshot on a
`NodeApi` behind `AccountTokenAuthority`, serves it on loopback, signs in as an operator
over `POST /v3/session`, reads it back over `GET /v3/snapshot` with the bearer token, and
asserts full equality with the original. It also checks `/v2/snapshot` still answers
`410 Gone` under the same token, so a reader of this file does not go looking for a v2
route that would round-trip anything.

## What was decided, and what was not

Correcting the verification table's "v2" to "v3" is a factual correction -- the interface
was renamed under GAP-130, the zero-loss criterion itself is unchanged -- not a widening
of the criterion. `verification-capability-table.md` is nonetheless human-owned for any
change (`agentic-workflow.md`), so the row is left **not gated**, awaiting the owner's
confirmation that this test checks the criterion the row states, per this table's own
rule that a row becomes a gate only on that confirmation and an entry in
`signatures.md`.

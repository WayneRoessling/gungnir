# A journal written before GAP-175

Written 2026-09-26 at commit `8cf37d4b` ("The owner's signatures of 2026-09-26: GAP-114's
fusion-async and GAP-156's one_step (#183)"), before `GlobalEntityId`'s written form
changed. It is for GAP-175 and D-101: the owner's decision that the identity is written as
RFC 9562 text from now on and that **both forms read**, so that every journal written since
GAP-069 keeps reading and nothing is rewritten. **Every identity in it is a UUID v7 written
as a 128-bit JSON number**, minted by `gungnir-identity` (`uuid::Uuid::now_v7`) and too wide
for a double to carry: read through a float, each would become a different entity. Nothing
here is real and no licence attaches.

## How it was written

By a temporary integration test in `gungnir-app`, marked `#[ignore]`, run once with
`cargo test -p gungnir-app --test gap175_fixture_gen -- --ignored`, with `GAP175_OUT` naming
the scratch data directory, and deleted before this
directory was committed, for the reason `../pre-uuid-v7/SOURCE.md` gives for its own: from
GAP-175 on the same code writes text, so a generator left here would claim to reproduce a
file it can no longer write.

The test drove two desktops (`AppState::with_config`, the default baseline) over one
scratch data directory, one after the other, as `gungnir-app/tests/identity_survives_a_restart.rs`
does: a scripted tracking service holding one confirmed track per step, published as
`TrackingEvent::TrackInitiated` and resolved by the desktop's own identity path on
`update::tick`, with a `ReplayClockAuthority` set before each. Each desktop ended with
`save_session`.

| Session | Mission time | What the test did | What the journal holds (sequence numbers) |
|---|---|---|---|
| 1 | 100 s | Track 1 at 1000 m east, 10 m/s | `TrackInitiated` (0); `IdentityEvent::Minted` track 1, entity `01a0df82-6bb1-75d1-9e86-4109de36bbde` (1); governance and health (2, 3) |
| 2 | 130 s | After a restart, track 0 at 1300 m east, 10 m/s: the same object | `TrackInitiated` (0); `IdentityEvent::Correlated` track 0 to the same entity, on similarity (1); governance and health (2, 3) |
| 2 | 131 s | Track 2 at 300 km east: a different object | `TrackInitiated` (4); `IdentityEvent::Minted` track 2, entity `01a0df82-6bc4-7f50-baee-df50c409b1c7` (5) |

The two entities, as the numbers the lines hold, are
2164528803482661774963721757477223390 and 2164528803505676211618302957697216967.

**Only the two session files were kept.** The desktop also wrote `1.mission.json` and
`2.mission.json`, which hold the default baseline with the scratch directory's absolute
path in it and no identity, and an empty `audit` directory.

## Files

| File | What it is | SHA-256 |
|---|---|---|
| `session-000000000001.jsonl` | Session 1's journal, 4 envelopes, copied unchanged from the scratch data directory | `74ea83b65c5080a95ee2c3c3df272aaae81ed9e974af395c7dccb590c7776273` |
| `session-000000000002.jsonl` | Session 2's journal, 6 envelopes, copied unchanged | `e82ae83afb7e0c64a5f7c9e36e4c6d3bdd840de7caa196d021cb3dfdc59c24b5` |

Neither file is edited or regenerated: a changed file is a different fixture, and the point
of this one is that it was written by the code as it stood before the change.
`gungnir-app/tests/pre_gap_175_journal.rs` reads it.

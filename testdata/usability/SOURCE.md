# Usability round 1: baseline and seed

Written 2026-09-06 for GAP-089 out of `docs/ux/usability-round-1-session.md` §3. Nothing
here is real: the positions are the Vell estuary of the vignettes at a plausible latitude,
the sensors and effectors are the laydown the vignettes describe, and the tracks are the
three the task cards name. No licence attaches.

| File | What it is |
|---|---|
| `round-1.json` | The session baseline: two radars, a point-layer battery with a reserve and an area-layer resource, the harbour (with a warning obligation to the port authority) and the plant as defended assets, one no-go fence, the harbour boom as a hazard. The desktop loads it through `GUNGNIR_CONFIG` like any baseline. Its validity is checked by `gungnir-app/tests/rehearsal.rs` |
| `round-1-seed.json` | The seed the rehearsal driver runs: T-039 appearing at once and going stale after 80 s (US-03); two drones inbound to the plant from 20 s (US-01); P-1181 against resource 2, marked not ready, at 30 s (US-02, refused on readiness); P-1183 against resource 1 at 45 s (US-01); six raw alerts from 55 s (US-05). The hash of this file is journaled at the start of every seeded session, and the scoring sheet records it |

To run a session: set `GUNGNIR_CONFIG` to `round-1.json`, start the desktop with
`--rehearsal testdata/usability/round-1-seed.json`, and read the status strip: it says
"rehearsal" with the seed's name for the whole session. A baseline naming a node backend
refuses the rehearsal.

The seed's schedule is in seconds from the first frame. A moderator who needs a task
sooner or later edits `at_s` and records the new hash on the sheet; the seed is data, not
code, and a changed seed is a different session.

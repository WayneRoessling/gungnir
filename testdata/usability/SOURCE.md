# Usability round 1: baseline and seed

Written 2026-09-06 for GAP-089 out of `docs/ux/usability-round-1-session.md` §3. Nothing
here is real: the positions are the Vell estuary of the vignettes at a plausible latitude,
the sensors and effectors are the laydown the vignettes describe, and the tracks are the
three the task cards name. No licence attaches.

**Held as of 2026-09-08: GAP-097 blocks every session below.** The live allocator
re-proposes an unchanged assignment every tick once a track and a ready resource
coexist, so the approval queue floods well past what either file scripts, on the desktop
and on a node alike. See `docs/mission/gap-analysis/gap-register.md` GAP-097 and
`docs/ux/usability-round-1-session.md`'s own hold notice. Nothing below runs until it
closes.

| File | What it is |
|---|---|
| `round-1.json` | The session baseline: two radars, a point-layer battery with a reserve and an area-layer resource, the harbour (with a warning obligation to the port authority) and the plant as defended assets, one no-go fence, the harbour boom as a hazard, and (added 2026-09-08 for US-15) two declared laydowns, `current` and `b`, their sensor and resource placements converted from the baseline's own geodetic positions through `gungnir-coord::Wgs84` against the baseline's own origin. The desktop loads it through `GUNGNIR_CONFIG` like any baseline. Its validity is checked by `gungnir-app/tests/rehearsal.rs` |
| `round-1-seed.json` | The seed the rehearsal driver runs: T-039 appearing at once and going stale after 65 s -- 20 s after P-1183 appears at 45 s, matching US-03's own card, corrected 2026-09-08 from an earlier 80 s that missed the card by 35 s; two drones inbound to the plant from 20 s (US-01); P-1181 against resource 2, marked not ready, at 30 s (US-02, refused on readiness); P-1183 against resource 1 at 45 s (US-01); six raw alerts from 55 s (US-05); and, added 2026-09-08 for US-06, six more point-layer plans (1201-1206) at 0, 5, 50, 58, 65 and 75 s so the queue reaches seven with two (1201, 1202) clearly nearer their 90 s expiry than the rest. The hash of this file is journaled at the start of every seeded session, and the scoring sheet records it |

To run a session: set `GUNGNIR_CONFIG` to `round-1.json`, start the desktop with
`--rehearsal testdata/usability/round-1-seed.json`, and read the status strip: it says
"rehearsal" with the seed's name for the whole session. A baseline naming a node backend
refuses the rehearsal. US-01, US-02, US-03 and US-05 all read on the same P-1181/P-1183
pair, so relaunch fresh for each rather than running them back to back in one process.

The seed's schedule is in seconds from the first frame. A moderator who needs a task
sooner or later edits `at_s` and records the new hash on the sheet; the seed is data, not
code, and a changed seed is a different session. **`tracks` and `plans` must each stay
sorted by ascending `at_s`** -- `rehearsal::tick` advances a single cursor through each
array and does not scan ahead, so moving one entry's `at_s` earlier without also moving
it earlier in the array leaves it stuck behind whatever the array still has in front of
it. `load_seed` refuses an out-of-order seed and names the field and the first pair out
of order, added 2026-09-08 after exactly this mistake was made while writing US-06's six
extra plans.

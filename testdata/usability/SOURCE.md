# Usability round 1: baseline and seed

Written 2026-09-06 for GAP-089 out of `docs/ux/usability-round-1-session.md` §3. Nothing
here is real: the positions are the Vell estuary of the vignettes at a plausible latitude,
the sensors and effectors are the laydown the vignettes describe, and the tracks are the
three the task cards name. No licence attaches.

**Hold lifted 2026-09-08: GAP-097 is closed.** The live allocator no longer re-proposes
an unchanged assignment every tick; `gungnir-app/tests/rehearsal.rs` exercises this same
seed and baseline against the fix directly. See `docs/mission/gap-analysis/gap-
register.md` GAP-097 and `docs/ux/usability-round-1-session.md`'s own note. Sessions may
resume; US-09 still separately needs a SAPIENT loopback fixture, tracked on its own.

| File | What it is |
|---|---|
| `round-1.json` | The session baseline: two radars, a point-layer battery with a reserve and an area-layer resource, the harbour (with a warning obligation to the port authority) and the plant as defended assets, one no-go fence, the harbour boom as a hazard, and (added 2026-09-08 for US-15) two declared laydowns, `current` and `b`, their sensor and resource placements converted from the baseline's own geodetic positions through `gungnir-coord::Wgs84` against the baseline's own origin. **One approach was added later the same day, also for US-15**: `sustainment::planning_rows` reports `NotComputed` for every laydown when a baseline declares no approach, so PN-16's whole options table read "not computed" and US-15's card asked for a number no session could produce. The axis declared is the upper Vell approach the laydown intents and the no-go fence already name, three points at 300 m -- an altitude, not sea level, because without a terrain model line of sight is tested against the ENU tangent plane and a ground-level axis reads as hidden past a few kilometres (`ApproachConfig::points`' own doc). It runs ~25 km out, past both radars' range, so the table reports a real gap rather than a flat zero: two segments, 7 000 m uncovered, the same for both laydowns. The desktop loads it through `GUNGNIR_CONFIG` like any baseline. Its validity, and both laydowns' coverage, are checked by `gungnir-app/tests/rehearsal.rs` |
| `round-1-seed.json` | The seed the rehearsal driver runs: T-039 appearing at once and going stale after 65 s -- 20 s after P-1183 appears at 45 s, matching US-03's own card, corrected 2026-09-08 from an earlier 80 s that missed the card by 35 s; two drones inbound to the plant from 20 s (US-01); P-1181 against resource 2, marked not ready, at 30 s (US-02, refused on readiness); P-1183 against resource 1 at 45 s (US-01); six raw alerts from 55 s (US-05); and, added 2026-09-08 for US-06, six more point-layer plans (1201-1206) at 0, 5, 50, 58, 65 and 75 s so the queue reaches seven with two (1201, 1202) clearly nearer their 90 s expiry than the rest. The hash of this file is journaled at the start of every seeded session, and the scoring sheet records it |
| `tools/sapient_loopback.py` | The SAPIENT loopback fixture US-09 needs (added 2026-09-08): a generic "always accept" SAPIENT sensor a moderator runs alongside the session, answering whatever `task_id` it receives with `Accepted` on the same connection. `round-1.json` has no `sapient_feeds` entry pointed at it yet -- that wiring is a session-setup step, not something the script does |

**`round-1-node.json` is not in this directory and never was.** US-04 and US-08 need a
baseline naming a node backend, and this file and the session document both once wrote
about one as though it were committed. It is not: the moderator writes it during that
task's dry run, from `round-1.json` with its `backend` and
`security.authentication.provider` sections replaced, as
`docs/ux/usability-round-1-session.md` §3 sets out. Nothing here generates it, and a
rehearsal seed cannot be installed against it by design.

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

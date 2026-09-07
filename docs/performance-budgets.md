# Performance budgets

End-to-end service-level objectives for a deployed Gungnir system, derived from
the five mission-scale scenarios in `scenario-crate-narrative.md` rather than from
component benchmarks alone (`gungnir-capabilities.md` §5.6, "Scalability &
performance budgets"). Component benchmarks (`../benches/README.md`) guard against
regression; these budgets say what the whole system must achieve.

Every number below was proposed on 2026-09-04 and confirmed by the owner the same
day as a **provisional gate** (D-04 in `mission/gap-analysis/decisions-needed.md`).
Each is marked with the scenario that exercises it; a value changes only with a
recorded reason, and none is enforced in `verification-capability-table.md` §2 until
its harness exists (gap GAP-056). The desktop harness landed on 2026-09-05 and what it
measured is in "What has been measured" below; the node and connectivity harnesses do
not exist, so every budget in those two sections remains unevidenced.

## Desktop (disconnected and connected profiles)

| Budget | Draft value | Exercised by | Notes |
|---|---|---|---|
| Frame rate with the 3D viewport open | 60 fps sustained, never below 30 fps | Scenario 4 (dense swarm, 200 tracks) | egui + three-d in one GL context; glyphs rebuilt only on change. |
| Per-frame `update()` time (ingest + poll + plan + journal) | p99 under 4 ms | Scenario 3 (three sensors, mismatched rates) | The DP solve moves off-thread if it ever exceeds this. |
| egui pass (2D panels) | p99 under 8 ms | Scenario 4 | Agreed 2026-09-04 with the `gungnir-ui` verification row; half the 60 fps frame, the rest for the viewport and `update()`. |
| `tracks()` and `is_healthy()` snapshot calls | p99 under 1 ms | Scenario 4 | Agreed 2026-09-04 with the `gungnir-tracking-service` row; cloned projection, never touches the pipeline. |
| Detection to on-screen track update | p99 under 250 ms | Scenario 3 | Source time to glyph update, embedded profile. |
| Journal append cost per frame | Under 1 ms for 50 envelopes | Scenario 2 (clutter, many ingest events) | Buffered JSON-lines writes. |
| Memory, steady state, 1 hour | Under 1.5 GB without point clouds | Scenario 5 (soak) | Excludes loaded terrain and point clouds. |
| Startup to first frame | Under 3 s with the default config | All | Includes runtime, journal open, services construction. |

## Service node (on-prem and cloud profiles)

| Budget | Draft value | Exercised by | Notes |
|---|---|---|---|
| Detection ingest throughput | 5,000 detections/s sustained per node | Scenario 4 | Through validation and quarantine, single tick loop. |
| Detection to event-stream publish | p99 under 150 ms on-prem, under 400 ms cloud | Scenario 3 | Node-side latency only; the desktop's display adds its own budget. |
| Event stream fan-out | 10 subscribed desktops without exceeding the above | Scenario 2 | `InProcessBus` broadcast plus the API transport. |
| Snapshot request | p99 under 50 ms for 500 tracks | Scenario 4 | `gungnir-api` v1 snapshot. |
| Journal durability | An accepted envelope is on disk within 100 ms | All | Fsync every envelope on the node; desktop journals buffered with fsync on session save and every 5 s (D-04). |
| Recovery time after restart | Under 30 s to serving with the last checkpoint | Scenario 5 | `gungnir-resilience` checkpoint replay. |

## Connectivity (connected profiles)

| Budget | Draft value | Notes |
|---|---|---|
| Link staleness visible to the operator | Within 2 s of the last heartbeat | PN-01 shows "heard N s ago" against a 2 s beat (D-23). |
| Fallback to embedded after link loss | Within four beats (about 7 s) of the last heartbeat | `HEARTBEAT_TIMEOUT` is derived from the beat, three misses tolerated (D-23). `gungnir-app` alert raised; `RemoteTrackingService` outbox starts. |
| Store-and-forward capacity | 100,000 detections per desktop | `gungnir_remote::OUTBOX_CAPACITY`; oldest dropped and counted beyond that. |
| Reconciliation after reconnect | Under 60 s for a 10-minute outage | `gungnir_resilience::reconcile`; conflicts reported, not resolved (open decision). |

## Numerical (tracking core)

These are already gates in `verification-capability-table.md` §1 and are repeated
here only so the budgets read as one set: zero PSD violations and zero NaN/Inf over
10⁵+ cycles (Scenario 5), and the per-row tolerances against the named oracles.

## What has been measured (2026-09-05)

The desktop tick harness exists: `gungnir-app/benches/app_tick.rs` for the `criterion`
measurement and `gungnir-app/tests/frame_budgets.rs` for the assertions (GAP-056). The
figures below are from this development machine, release profile except where noted.

| Budget | Value | Measured | Gate? |
|---|---|---|---|
| Per-frame `update()` | p99 under 4 ms | ~20 µs per frame (300 frames in 6.1 ms); debug-profile p99 533 µs, worst 3.32 ms | **No.** The tracking stage is a stub, so this is a floor, not the budgeted quantity. |
| `tracks()` / `is_healthy()` | p99 under 1 ms | ~0.6 ns each | **No.** The snapshot is empty while `PIPELINE_IMPLEMENTED` is false, so it is not "at Scenario 4 track counts". |
| Journal append per frame | under 1 ms for 50 envelopes | **157 µs release; median 400 µs debug over 9 runs on this machine, 1.84 ms debug on a shared CI runner** | **Yes**, since GAP-085 — **in the release profile only** since 2026-09-07; see the note below. |
| Startup to first frame | under 3 s | 16.5 ms release, 12.5 ms debug (excludes eframe window creation) | **Yes.** |

Three notes on those figures.

**The journal budget was the one failure, and it is fixed.** Before GAP-085,
`FileEventJournal::append` opened the session file, wrote one line, and closed it once
per envelope: 5.7 ms release, 8.3 ms debug for fifty. The "Buffered JSON-lines writes"
note in the table above and the D-04 policy in `../ARCHITECTURE.md` §10 item 19 describe
what it should have been doing, and now does. The per-frame `update()` figure improved
with it, from a 533 µs debug p99 to 40.8 µs, because the journal was that path's
dominant term. Which profile is measured matters: this budget is stated for the buffered
**desktop** profile. Under the node profile fifty envelopes are fifty fsyncs, and the
node is measured against its own budget instead, "an accepted envelope is on disk within
100 ms".

**The journal budget is asserted in release only (owner decision, 2026-09-07).** The
budget is unchanged at 1 ms for fifty envelopes; what changed is which build it is
asserted against. `cargo test` builds in debug, and the first CI run this repository ever
had (GAP-061) measured a median of **1.844781 ms** on a GitHub Actions runner — worse
than the 1.0681 ms a Windows development machine gives. Both are debug figures being held
to a budget whose evidence is 157 µs in release, so the failure was about the build
profile rather than about `FileEventJournal`.

**Why it is slow in debug was measured afterwards, and it is not the I/O** (GAP-092). The
first reading of this attributed it to a shared runner's throttled I/O. Splitting `append`
into its two halves says otherwise: on a P-core, `serde_json::to_string` alone measures
~500 µs, the file write 15-50 µs, and the whole append ~515 µs. **The encode, compiled at
`opt-level = 0`, is nineteen twentieths of it and the write is about a twentieth.** Three
further measurements agree. Running the same nine-run measurement alternately against
`%TEMP%` on C: and `target/` on D: — two different filesystems — gives 514.9 / 516.5 /
513.8 µs against 518.1 / 506.7 / 508.4 µs, indistinguishable. Pinning the test to each of
the twenty logical CPUs of an 8 P-core, 12 E-core development machine splits it 8 fast
(~760-990 µs) from 12 slow (~1,650-2,045 µs) on the P/E boundary exactly, reproducing the
whole spread with no I/O involved. And in release the same path measures 45 µs on a
P-core and 157 µs on an E-core, the second of which is the recorded release figure.

The decision is unaffected — a debug build of fifty `serde_json` encodes takes about a
millisecond on ordinary hardware, whatever the disk does, so a 1 ms budget asserted in
debug has no margin anywhere. It is recorded because a right decision resting on a wrong
reason invites the wrong follow-up: chasing runner I/O, or moving the test's scratch
directory off the system temp directory, neither of which would change this number.

**One recorded figure disagrees with itself.** The 2026-09-05 debug measurement appears
three times: "median 400 µs debug over 9 runs" in the table above, **679 µs** in the gap
register's GAP-085 closing action, and **about 665 µs** in the test's own doc comment.
Today's debug P-core median is ~520 µs. Against 665-679 µs there is nothing to explain;
against 400 µs there appears to be a 2.6x regression that does not exist. The 400 µs is
the outlier of the three, and is best read as a best-case P-core sample rather than the
median it is labelled. It is left in place rather than silently corrected, because a
measurement is not amended by a later measurement's author.

`gungnir-app/tests/frame_budgets.rs` still measures and prints the figure on every
profile, so the debug number stays visible; the assertion applies in release.
**`ci.yml` runs `cargo test -p gungnir-app --test frame_budgets --release` as a separate
step, and that is where the gate is enforced.** That step is load-bearing: a release-only
budget that no job runs in release would be a gate that cannot fail, which would be worse
than the flaky one it replaced. The alternatives considered and rejected were widening the
budget, scaling it per profile, and skipping the test — each makes the gate pass by making
it claim less.

**Startup got slower, and that is a measurement artifact rather than a regression.** The
`criterion` startup group builds a desktop per iteration, so each one now pays the
preceding iteration's fsync-on-drop, which did not exist before the journal held a file
open. A real startup does not pay it. Either way the figure is more than a hundred times
inside its 3 s budget, so it was not worth chasing further.

One harness detail worth keeping: a recorded scenario must be replayed on
`gungnir_time::ReplayClockAuthority`, not the wall clock. `AppState::new` installs a
`WallClockAuthority` whose `now()` is Unix seconds, while a scenario's source times are
mission seconds from zero, so on the wall clock every detection in the file is already
due on the first tick and the whole feed drains in one frame. Measured that way a frame
takes 249 ms and means nothing.

## How these are measured

- Desktop budgets: a `criterion` harness around `gungnir-app::update::tick` fed by
  `gungnir-scenario` output, plus frame timing from `tracing` spans in the viewport.
  **Built (GAP-056): `gungnir-app/benches/app_tick.rs`, with the assertable budgets in
  `gungnir-app/tests/frame_budgets.rs`.**
- Node budgets: load generation through the recorded-feed adapter at scenario rates.
  **Built 2026-09-05 (GAP-056): `gungnir-node/benches/node_load.rs`.** It is a separate
  harness from the desktop's not because the code differs but because **the durability
  profile does**: D-04 gives the node `SyncEveryEnvelope` and the desktop
  `Buffered { 5 s }`, so fifty envelopes are fifty fsyncs on one and none on the other,
  and reporting the desktop's number as a node figure would be wrong by two orders of
  magnitude. First measurements, with the tracking stage doing nothing: **~4.7 M
  detections/s** through the gateway against a 5,000/s budget, and **~0.26 ms per fsynced
  envelope** against the 100 ms durability budget.
- Connectivity budgets: an integration test that runs a node and a desktop in one
  process, cuts the transport, and measures fallback and reconciliation. **Built
  2026-09-05 (GAP-056): `cutting_the_transport_leaves_the_desktop_detached_and_honest` in
  `gungnir-remote/tests/transport.rs`.**

**A budget the implementation could not meet, found by building that last one, and
amended as D-23 (2026-09-06).** The original row read "fallback under 2 s from the last
successful heartbeat" beside a 10 s beat and an independent 35 s timeout. When the node goes
*down* the socket closes and the desktop is detached in under a second. When the node goes
*silent with the socket open* -- a wedged process, a suspended machine, a NAT dropping the
flow -- nothing closes, and the desktop waited 35 s. That was the case the budget existed
for.

Tightening the timeout alone could not fix it: at a 10 s beat, a 2 s timeout declares a
healthy link dead between beats. And meeting 2 s by force -- a half-second beat and a 2 s
timeout -- would have made every 2 s stall on an intermittent link a fallback and a full
reconnect (sign-in, snapshot, resubscribe), which is churn on exactly the links the
connected profile exists for.

D-23 split the budget into the two things it was conflating. **Visibility**: the operator
can see the picture going stale, which needs no timeout at all -- PN-01 shows "heard N s
ago" ticking up in real time, and with the beat at 2 s that number means something (at
10 s it would have read "9 s" on a perfectly healthy quiet link). **Declaration**: the
desktop decides the link is gone within four beats, tolerating three misses, so one stall
is not a reconnect. The timeout is now *derived* from the beat in code rather than being a
second constant, because two independently edited constants is precisely how this conflict
was created. Pinned by `the_heartbeat_meets_the_amended_connectivity_budget`, which asserts
the relationship rather than the numbers.

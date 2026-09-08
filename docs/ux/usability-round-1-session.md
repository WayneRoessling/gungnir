# Usability round 1 on the built panels: session script and scoring sheet

**Held as of 2026-09-08: no session in this document runs, seeded or node-backed, until
GAP-097 closes.** Reviewing this document against the code two days after it was written
found that three of its blocking gaps had since closed (GAP-050, GAP-057, GAP-041,
GAP-004), which unblocks US-04, US-08, US-09 and part of US-15 -- but tracing the seed to
confirm that before rewriting a single task card found instead that the live allocator
now re-proposes an unchanged assignment every tick once GAP-029 closed (2026-09-06),
flooding the approval queue at the redraw cadence, on the desktop and on a node alike.
`docs/mission/gap-analysis/gap-register.md` GAP-097 has the trace and the fix. Round 1 is
now **fourteen** tasks, §2, and §3 through §5 describe all fourteen, but do not run any
of them -- seeded or node-backed -- until GAP-097 is closed and `gungnir-app/tests/rehearsal.rs`'s
plan-count assertion is tightened back to exact per its own comment.

Decided 2026-09-06 by the owner (D-28, GAP-074): round 1 runs against the **rendered
panels**, not the wireframes, one participant per role, and its results become the
MOP-37 target proposal. This document is what a moderator needs on the day. What the
owner supplies is in §1; what the software cannot yet show a participant is in §2 and is
stated rather than worked around.

## 1. What the owner supplies

| Item | Needed by | Note |
|---|---|---|
| One participant per role, eight in all; operators and supervisors first | session scheduling | From `../mission/mission-analysis.md` §11's reviewers or a customer's staff. **Supplied 2026-09-06: the owner, for all eight roles.** One person who also wrote the requirements is not eight users, so every measure from this round is provisional by §6's rule; the report (`reports/round-1-2026-09-06.md`) says so on each line |
| Dates, and one moderator per session (a second person scribes) | scheduling | 75 minutes per session, §4; the operator, supervisor and sensor manager sessions carry a second slot the same day for US-04/US-08/US-09's node-backed setup once GAP-097 closes. **Supplied: 2026-09-06.** A single participant moderating themself has no scribe; the journal is the clock for decision latency and the sheet is filled after each task |
| The consent, recording and data-handling process of the moderator's organisation | before the first session | No real operational data; no participant names in this repository. The scoring sheet identifies a participant by role and session number only |
| A machine with the desktop built from the tested revision, and the revision recorded on the sheet | each session | `cargo run -p gungnir-app`, with the session baseline from §3 |

## 2. Which tasks can run today, and which cannot

The test plan's sixteen tasks (`usability-test-plan.md` §4) were written against
wireframes. Against the built desktop, on 2026-09-08, they fall into four groups. The
grouping is a statement of what the binary does, not of what the panels look like, and it
has already changed once (2026-09-06 to 2026-09-08) as engineering closed gaps this
document did not have -- it will very likely change again before the eight sessions run.

| Group | Tasks | Why | What unblocks the rest |
|---|---|---|---|
| **A. Runs now, single desktop, no seed** | US-10 (state a requirement), US-11 (replay a recorded session and find a decision), US-12 (raid summary and the MOE-04 trace), US-13 (an invalid baseline in the editor) | The write path is wired and the panel is built; US-11 and US-12 need a recorded session, which §3's seed produces | Nothing |
| **B. Needs the seeded session, single desktop** | US-01, US-02, US-05, US-06 (plans in the queue), US-03 (a track that goes stale), US-14 (evidence on a track) | No plan or track appears on an unseeded desktop from a laptop's own sensors, and a session needs the *same* picture in front of every participant regardless. GAP-089's `--rehearsal` driver puts the seed's tracks and plans through the real submit path | **GAP-097** must close first: the live planner (real since GAP-029, 2026-09-06) now re-proposes an unchanged assignment every tick on top of the seed's own scripted plans, flooding the queue |
| **D. Newly unblocked (2026-09-06/08), each needs a setup beyond the single `--rehearsal` flag** | US-04 (Detached strip, continue under delegation), US-08 (KAL cell reconnects, one conflict) -- both need a real `gungnir-node` signed into and then lost; US-09 (re-task a sensor, coverage before/after, commit) -- needs a live SAPIENT acknowledgement loop the round-1 baseline does not yet configure; US-15 (compare laydowns, submit with a rehearsal) -- needs PN-16's own scenario rehearsal, single-process, no node | GAP-050 (failover and reconciliation), GAP-057 (desktop and node sign-in) and GAP-041/GAP-004 (sensor tasking transport and acknowledgement) all closed by 2026-09-07; GAP-045 landed PN-16's real rehearsal 2026-09-08 | US-04/US-08/US-09 run the same live planner as group B and are held on **GAP-097** too. US-09 additionally needs a SAPIENT loopback fixture nobody has written yet (§3). US-15 is not gated by GAP-097's queue flood as directly, but its rehearsal replay drives the same `update::tick`, so its own decision counts are suspect until GAP-097 closes too |
| **C. Blocked by an unwired write or an undesigned control** | US-07 (set the area layer to Hold: `SET_CONTROL_STATUS` exists as an authorization constant with no caller anywhere in the UI -- no control-status write reaches the desktop), US-16 (accept a coverage gap with a warning obligation and an expiry: GAP-087's own text says the gap-acceptance control "is neither designed anywhere," not GAP-068 as an earlier draft of this table said -- GAP-068 (roles in code) closed 2026-09-05 and was never this task's blocker) | The panel draws nothing for the write, by the rule that a control which does not do the thing is worse than no control | An unfiled design decision for the gap-acceptance control (US-16) and a control-status write path (US-07); neither is scoped yet |

**Round 1 is therefore fourteen tasks (groups A, B and D), once GAP-097 closes.** Group
C's two tasks are round 2's, and the report says so per task rather than scoring a task
the software could not present. The measures group C would have fed (decision latency
under a status change, the gap-acceptance decision itself) are reported as **not
measured**, never as a value.

## 3. The session baseline and the seed

- **Baseline.** The Vell estuary laydown from the vignettes: two radars, one
  interceptor battery at the point layer with a reserve, the harbour and the plant as
  defended assets, one no-go geofence, the harbour boom as a hazard, and (added
  2026-09-08 for US-15) two declared laydowns -- `current` (the deployment as sited) and
  `b` (the area-layer battery moved toward the upper Vell approach, ahead of R2's
  scheduled maintenance window). It lives at `testdata/usability/round-1.json`
  (GAP-089, landed), with a `SOURCE.md` beside it.
- **Seed.** GAP-089's driver reads `testdata/usability/round-1-seed.json` and, on a
  schedule from session start, shows the tracks the tasks name (T-039 and the two inbound
  drones) from a scripted picture -- not from detections, because no pipeline turns a
  detection into a track -- and submits the plans (P-1181 refused on readiness, P-1183
  requiring approval, and six more from 2026-09-08 so US-06's queue reaches seven with
  two near expiry) through the same policy chain and queue a live planner would.
  It stamps every journal record it causes as a rehearsal, so a seeded session can never
  be read as an operation, and PN-01 shows "rehearsal" for its duration. **The seed's
  `tracks` and `plans` arrays must stay sorted by ascending `at_s`** -- `rehearsal::tick`
  advances a single cursor through each and does not scan ahead, so an earlier-timed
  entry placed after a later one is silently skipped until the entries in front of it
  come due. `load_seed` refuses an out-of-order seed and names the field, added
  2026-09-08 after this document's own author hit exactly that mistake while adding
  US-06's six plans.
- **Recorded session for US-11 and US-12.** The moderator runs the seed once before the
  first session and keeps the resulting journal; every analyst participant replays that
  one journal, so their timings are comparable.
- **US-01/US-02/US-03/US-05 each need their own relaunch.** All four read on the same
  P-1181/P-1183 pair, and the first of them to run will have already decided P-1183 and
  emptied it from the queue. Relaunch `gungnir-app` (same `GUNGNIR_CONFIG` and
  `--rehearsal` flags) fresh for each of the four operator cards rather than running
  them back to back in one process.
- **US-04 and US-08 need a real node, not the rehearsal flag.** `rehearsal::install`
  explicitly refuses a baseline whose backend is not embedded (`RehearsalError::NotEmbedded`),
  because a rehearsal against a shared node picture is not a rehearsal. Bring up a
  second baseline naming the node profile (`backend: {"kind": "remote", "endpoint":
  "http://127.0.0.1:7410"}`, `security.authentication.provider: {"kind":
  "local-accounts", "accounts_path": "accounts.json"}`), provision one operator account
  and start the node before the session:

  ```bash
  printf '%s' 'the passphrase' | gungnir-node account add accounts.json 7 operator
  GUNGNIR_TOKEN_KEY=<a signing key> gungnir-node round-1-node.json
  ```

  Start `gungnir-app` against the matching desktop baseline, sign in through PN-20 with
  operator 7, and confirm PN-01 shows the node link established before reading the
  warm-up card. For US-04, kill the node process (`taskkill /PID <pid> /F` on Windows)
  at the trigger point and read PN-01/PN-18 for what the strip and the outbox say. For
  US-08, decide a plan locally while the node is down, restart the node with a
  contradicting decision already on its own record (`gungnir-app/tests/failover_e2e.rs`
  shows the shape), and resolve the one conflict PN-18 reports once the link is restored.
  **This two-process setup needs its own dry run before the first participant**; nothing
  in the codebase scripts it the way `--rehearsal` scripts the single-desktop tasks.
- **US-09 needs a live SAPIENT acknowledgement loop the round-1 baseline does not yet
  configure.** Tasking a sensor and seeing the request through PN-10 works from the
  baseline's own sensor list alone, but "commits" (an acknowledgement, not just an
  issued command) needs something answering the wire `task_id`
  `SapientTaskAdapter::issue` mints -- `gungnir-app/tests/sapient_task_ack.rs` proves the
  mechanism end to end on a single desktop, no node required. GAP-004 landed the node's
  own half of the same loop on 2026-09-08 (`gungnir-node/src/main.rs::bind_sapient_feeds`,
  `TcpSapientSource::sink`), so a node-backed US-04/US-08 setup could in principle carry
  US-09 too. Either way, round-1.json (desktop) and round-1-node.json (node) have no
  `sapient_feeds` section today, and nothing plays the sensor's side of that exchange
  interactively -- neither path is wired to a real or simulated SAPIENT sensor. Someone
  needs to write a small loopback fixture (a local TCP listener that echoes back
  `Accepted` for whatever `task_id` it receives) before this task is session-ready;
  until then, "commits" is not demonstrable and the moderator should treat US-09 as
  **not measured** rather than script around it.
- **US-15 runs PN-16's own rehearsal, single desktop, no node, no `--rehearsal` flag.**
  Pick a scenario in PN-16's picker (any of the ten `testdata/tracks/samples/TT-0N-sample/`
  fixtures), select laydown `b` in the options table, press Run, and read the rehearsal
  section's tracks-formed and decisions-raised counts once it finishes. This is GAP-045's
  mechanism, not GAP-089's, and it drives `update::tick` internally the same number of
  times the fixture has detections for -- so its own decision counts are exposed to
  GAP-097 exactly as the seeded tasks are, until that closes.

## 4. Moderator script

**This script covers groups A and B (ten tasks, one desktop, one seed) and US-15 (one
desktop, PN-16's own rehearsal).** US-04, US-08 and US-09 need the setups §3 describes
and do not fit this 75-minute slot on top of the other nine or ten: schedule each
operator/supervisor session's node-backed cards (US-04, US-08) as a second slot the same
day, after the seeded slot below, with the node already up per §3; schedule US-09
separately once its loopback fixture exists.

Times are cumulative. The moderator reads the bold lines aloud and does not help until
a task is marked "not completed"; the scribe keeps the sheet.

| At | Step | Script |
|---|---|---|
| 0:00 | Briefing | **"You are the [role] on the Vell estuary watch. Here is your delegation card. The system will show you what it knows and what it does not; when it says it cannot do something, that is the system being honest, not broken."** The nine principles, one sentence each, from `README.md`. |
| 0:10 | Warm-up | **"Find track T-039 and tell me what evidence the system has for it. Then acknowledge the information alert on the right."** Not scored; the moderator may help. |
| 0:15 | Tasks | The role's task cards from §5, in order. For each: read the card, start the clock at the moment the triggering event is visible, stop it at the success criterion or at "not completed". Think-aloud throughout. |
| 0:55 | Questionnaire | NASA-TLX raw (the six scales, 0 to 100, unweighted); the confidence question (§6); open comments. |
| 1:05 | Debrief | **"What did the system tell you that you did not believe? What did you need that it did not show?"** Verbatim quotes to the sheet. |
| 1:15 | End | The moderator records the desktop revision and the seed file's hash on the sheet, and copies the session journal to the report folder. |

## 5. Task cards

One card per task; the moderator reads only the **task** line. The rest is the scribe's.

| Card | Role | Task (read aloud) | Trigger the clock starts on | Success | Panel | Group |
|---|---|---|---|---|---|---|
| US-01 | Operator | "Two drones are inbound to the plant. Decide on the plan the system proposes, within your delegation." | P-1183 appears in PN-06 | Accepted through PN-07 with the verdict read aloud; latency under 0.3 of time remaining | PN-06, PN-05, PN-07 | B |
| US-02 | Operator | "Plan P-1181 was refused. Tell me why." | P-1181 shown as denied | The reason (resource not ready) stated from PN-05 within 10 s | PN-06, PN-05 | B |
| US-03 | Operator | "Decide on P-1183." (the seed makes T-039 stale 20 s in) | The stale label appears | Notices the label; does not accept while stale; says what they will do | PN-03, PN-07 | B |
| US-05 | Operator | "Decide on P-1183." (the seed raises six raw alerts 10 s in) | The first alert appears | The decision completes first; the one incident is acknowledged afterwards | PN-08, PN-07 | B |
| US-06 | Supervisor | "The queue is filling. Make sure nothing expires undecided." | The queue reaches seven with two expiring | Expiring items handled first; nothing expires; PN-17's counts read aloud | PN-06, PN-17 | B |
| US-10 | Sensor manager | "Leyla's cell asks for radar time over the upper estuary. Record the request and its cost." | Card read | A requirement stated in PN-15 against an asset with a priority; the supervisor's concurrence noted in the text | PN-15 | A |
| US-11 | Analyst | "Find the decision on P-1183 in last night's record and mark it." | Card read | Decision found in PN-12 within 2 min; the finding recorded in PN-13's review | PN-12, PN-13 | A |
| US-12 | Analyst | "Produce the raid summary and show me where MOE-04 comes from." | Card read | The report exported with its marking; MOE-04's line and its journal reference found | PN-13 | A |
| US-13 | Administrator | "Apply this baseline." (the file has a zero-capacity resource) | Card read | The validation failure found at the resource; apply stays refused; the revision rule explained | PN-14 | A |
| US-14 | Intelligence analyst | "Is the ISR UAS hostile? Tell me what the evidence says." | T-039's evidence shown | The engine's decision read from PN-04 (declared, needs an operator, or unknown with its reason) and the evidence lines named; the participant states that there is no designation control and why | PN-04 | B |
| US-04 | Operator | "At 02:03 the strip shows Detached." (the moderator kills the node at this cue) | The node link drops and PN-01 shows Detached | States which backend is now live (embedded fallback) and what is queued in the outbox from PN-18; says they can continue deciding under delegation while disconnected | PN-01, PN-18 | D |
| US-08 | Supervisor | "The KAL cell reconnects with one conflict." (the moderator restarts the node with a contradicting decision on its record) | PN-18 shows a reconciliation due | Resolves the one conflict through PN-18 (keep this desktop's decision or the node's) with the role-rank arbitration explained; presses switch-back once the node answers | PN-18 | D |
| US-09 | Sensor manager | "R1 is lost at 01:50. Re-task R2 to search, with coverage shown before and after." | Sensor 1 stops reporting | Commands R2 to Search through PN-10; states the coverage difference from PN-11; commits once acknowledged; reports the remaining gap | PN-10, PN-11 | D |
| US-15 | Planner | "Compare laydowns A and B and submit B with its rehearsal." | Card read | Both laydowns' coverage read from PN-16's table; laydown B selected and previewed in the viewport; the maintenance-window gap named; a scenario run and its rehearsal record read before submitting | PN-16, PN-11 | D |

Group C's cards (US-07, US-16) are held for round 2 and are not read in round 1.

## 6. Scoring sheet

One row per task per participant. Copy the table into the round's report; a field the
session could not produce is written **not measured**, never left blank and never
estimated.

| Field | Values | Source |
|---|---|---|
| Session | number; role; date; desktop revision; seed hash | moderator |
| Task | the card id | |
| Completion | `completed` / `with a hint` / `not completed` | scribe |
| Latency (s) | seconds from the trigger to success; for US-01 also as a fraction of time remaining | scribe's clock; the journal's `ApprovalRequested` to `Decided` for decisions |
| Errors | count, and each one named (accept for reject; wrong track; above delegation) | scribe and the audit panel afterwards |
| Keyboard only | `yes` / `no` / `not attempted` | scribe |
| TLX raw | six values 0 to 100 and their mean | questionnaire, after all tasks |
| Confidence | 1 to 5: "the system's state matched my understanding" | questionnaire |
| Quotes | verbatim | scribe |
| Not measured | the measures this task could not feed, and why (group C, or a seed fault) | moderator |

**The MOP-37 proposal** is computed per measure across the eight sessions as the
median and the worst case, and proposed to the owner in the round's report as
"target = the round-1 median, floor = the round-1 worst case", per measure, with the
sample size beside each. A measure with fewer than four sessions behind it is
proposed as **provisional** and says so.

## 7. Report template

`docs/ux/reports/round-1-YYYY-MM-DD.md`, one per round:

1. Sessions run: role, date, revision, seed hash; sessions planned and not run, with
   the reason.
2. The scoring sheet, all rows.
3. Per task: completion, latency, errors, TLX, confidence, quotes.
4. Per principle: whether the built panels made it visible, with the quote that shows
   it did or did not.
5. Findings ranked (blocks the task, slows it, cosmetic), each with the panel and the
   change proposed; engineering findings go to the gap register, design findings to
   the wireframes and the test plan.
6. The MOP-37 proposal table, §6.
7. Group C: the two tasks not run (US-07, US-16) and the gaps that unblock them.

## Traceability

GAP-074, GAP-089, GAP-097; GAP-050, GAP-057, GAP-041, GAP-004 (group D's blockers,
closed); GAP-045, GAP-087 (PN-16's rehearsal, US-15); D-28; MOP-37
(`../mission/capabilities/measures-catalogue.md`); MOE-04; `usability-test-plan.md` §1
to §5; `information-architecture.md` for the panel ids; `task-analysis/` for the roles'
tasks; principles 1 to 9 in `README.md`.

# Usability round 1 on the built panels: session script and scoring sheet

Decided 2026-09-06 by the owner (D-28, GAP-074): round 1 runs against the **rendered
panels**, not the wireframes, one participant per role, and its results become the
MOP-37 target proposal. This document is what a moderator needs on the day. What the
owner supplies is in §1; what the software cannot yet show a participant is in §2 and is
stated rather than worked around.

## 1. What the owner supplies

| Item | Needed by | Note |
|---|---|---|
| One participant per role, eight in all; operators and supervisors first | session scheduling | From `../mission/mission-analysis.md` §11's reviewers or a customer's staff. **Supplied 2026-09-06: the owner, for all eight roles.** One person who also wrote the requirements is not eight users, so every measure from this round is provisional by §6's rule; the report (`reports/round-1-2026-09-06.md`) says so on each line |
| Dates, and one moderator per session (a second person scribes) | scheduling | 75 minutes per session, §4. **Supplied: 2026-09-06.** A single participant moderating themself has no scribe; the journal is the clock for decision latency and the sheet is filled after each task |
| The consent, recording and data-handling process of the moderator's organisation | before the first session | No real operational data; no participant names in this repository. The scoring sheet identifies a participant by role and session number only |
| A machine with the desktop built from the tested revision, and the revision recorded on the sheet | each session | `cargo run -p gungnir-app`, with the session baseline from §3 |

## 2. Which tasks can run today, and which cannot

The test plan's sixteen tasks (`usability-test-plan.md` §4) were written against
wireframes. Against the built desktop, on 2026-09-06, they fall into three groups. The
grouping is a statement of what the binary does, not of what the panels look like.

| Group | Tasks | Why | What unblocks the rest |
|---|---|---|---|
| **A. Runs now** | US-10 (state a requirement), US-11 (replay a recorded session and find a decision), US-12 (raid summary and the MOE-04 trace), US-13 (an invalid baseline in the editor) | The write path is wired and the panel is built; US-11 and US-12 need a recorded session, which §3's seed produces | Nothing |
| **B. Needs a seeded session** | US-01, US-02, US-05, US-06 (plans in the queue), US-03 (a track that goes stale), US-14 (evidence on a track) | No plan appears in the live desktop: the allocator is `NotImplemented` (GAP-029, Area A) and the tracking pipeline is not implemented (GAP-011), so the queue and the track table are empty until something puts a plan and a track there. Tests do it through `ApprovalWorkflow::submit_for_approval` and `TrackingService::submit_detection`; a participant cannot | **GAP-089**: a session seed that drives those two surfaces from a scenario file, marked as a rehearsal in the journal |
| **C. Blocked by an unbuilt panel or an unwired write** | US-04 (the Detached strip on link loss: a node now verifies a sign-in (GAP-057's node half, 2026-09-06), so the link and its loss can be shown, but "continues under delegation" after detachment is GAP-050's fallback, unbuilt), US-07 (set the area layer to Hold: no control-status write reaches the desktop), US-08 (PN-18, unbuilt, GAP-050), US-09 (re-task a sensor: the command is recorded against a control endpoint and no transport carries it, GAP-003), US-15 (PN-16, unbuilt, GAP-087), US-16 (accept a coverage gap: the commander summary reports the acceptance list as unavailable, GAP-068's write half) | The panel draws nothing for the write, by the rule that a control which does not do the thing is worse than no control | The named gaps |

**Round 1 therefore runs groups A and B**, ten tasks. GAP-089 landed the same day this
was written, so nothing waits on engineering; the four group-A tasks need no seed at all. Group C's six tasks are round 2's, and the report says so
per task rather than scoring a task the software could not present. The measures that
group C would have fed (decision latency under a status change, the reconnection
confidence rating) are reported as **not measured**, never as a value.

## 3. The session baseline and the seed

- **Baseline.** The Vell estuary laydown from the vignettes: two radars, one
  interceptor battery at the point layer with a reserve, the harbour and the plant as
  defended assets, one no-go geofence, the harbour boom as a hazard. It lives at
  `testdata/usability/round-1.json` (GAP-089, landed), with a `SOURCE.md` beside it.
- **Seed.** GAP-089's driver reads `testdata/usability/round-1-seed.json` and, on a
  schedule from session start, shows the tracks the tasks name (T-039 and the two inbound
  drones) from a scripted picture -- not from detections, because no pipeline turns a
  detection into a track -- and submits the plans (P-1181 refused on readiness, P-1183
  requiring approval) through the same policy chain and queue a live planner would.
  It stamps every journal record it causes as a rehearsal, so a seeded session can never
  be read as an operation, and PN-01 shows "rehearsal" for its duration.
- **Recorded session for US-11 and US-12.** The moderator runs the seed once before the
  first session and keeps the resulting journal; every analyst participant replays that
  one journal, so their timings are comparable.

## 4. Moderator script

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

Group C's cards (US-04, US-07, US-08, US-09, US-15, US-16) are held for round 2 and
are not read in round 1.

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
7. Group C: the six tasks not run and the gaps that unblock them.

## Traceability

GAP-074, GAP-089; D-28; MOP-37 (`../mission/capabilities/measures-catalogue.md`);
MOE-04; `usability-test-plan.md` §1 to §5; `information-architecture.md` for the panel
ids; `task-analysis/` for the roles' tasks; principles 1 to 9 in `README.md`.

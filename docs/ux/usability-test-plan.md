# Usability test plan

Status: first draft 2026-09-04; **round 1 re-planned onto the built panels 2026-09-06 (D-28, §7)**. Scenario tasks per role from the vignettes, the
measures (MOP-37), participants, protocol, and reporting.

**Sequencing decision, 2026-09-05.** The owner chose to start Area D (the interface
panels) without waiting for round 1, accepting the rework risk explicitly: round 1 is
run on wireframes precisely so that layout and flow problems are found before panels are
built, so any finding from a later session now lands as rework on implemented panels
rather than as an edit to a drawing. Recorded here so the risk reads as accepted rather
than overlooked.

A round 1 session with end users was **begun on 2026-09-05 and is not
complete**: it does not yet cover one participant per role, has no per-round report, and
therefore yields no MOP-37 target proposal. The measures below remain unset, and GAP-074
stays open. One heuristic walkthrough by the drafting agent has been recorded below;
apart from the incomplete round 1, no other work with participants has
run yet, so the plan's acceptance criterion ("run at least once on wireframes with
recorded results") is met only for the expert walkthrough, and that is stated
plainly.

## 1. Measures (MOP-37, targets to be set from the baseline sessions)

| Measure | Definition | Collected by |
|---|---|---|
| Time to acknowledge | seconds from an alert or incident appearing to its acknowledgement | journal (`AlertTransition` times) or the moderator's clock on paper |
| Decision latency | seconds from a plan appearing in the queue to the recorded decision, as a fraction of time remaining (aligns with MOE-04) | journal (`ApprovalRequested` to `Decided`) |
| Error rate | wrong-control errors (accept when reject intended, wrong track selected, decision above delegation attempted) per task | observation and the audit log |
| Task completion | completed without help, with a hint, or not completed | observation |
| Workload rating | NASA-TLX raw score after each scenario | questionnaire |
| Confidence rating | participant's 1 to 5 confidence that the system's state matched their understanding (backend, staleness, verdict) | questionnaire |
| Keyboard-only completion | tasks completable without a mouse | observation (accessibility) |

## 2. Participants

- One per role for the first round (eight), from the subject-matter reviewers named
  in `../mission/mission-analysis.md` §11 or from a customer's staff when available;
  operators and supervisors are the priority.
- Two rounds. **Round 1 runs on the built panels (D-28, 2026-09-06)**, not on the
  wireframes: seventeen of twenty panels are built and the wireframes are behind the
  code, so a wireframe round would have found problems in drawings the panels no longer
  match. Round 2 runs on the panels that were unbuilt or unwired at round 1 (§7,
  group C). The wireframe round the first draft planned is not run and is not
  claimed.
- Consent, recording, and data handling per the test moderator's organisation; no
  real operational data.

## 3. Protocol

1. Briefing (10 min): the setting (the Vell estuary), the role, the delegation card,
   the principles in one sentence each.
2. Warm-up (5 min): select a track, read its evidence, acknowledge an Info alert.
3. Scenario tasks (40 min): the tasks below for the role, in order, think-aloud;
   the moderator does not help until a task is marked not completed.
4. Questionnaire (10 min): TLX, confidence, open comments.
5. Debrief (10 min).

The scenario is VG-01 at 01:41 with VG-07's degradation at 01:50 and VG-10's link
loss at 02:03 layered in, so one story exercises every live task.

## 4. Scenario tasks

| Id | Role | Task (from the vignettes) | Success | Wireframes |
|---|---|---|---|---|
| US-01 | Operator | Plan P-1183 appears with two drones inbound to OPS; decide within your delegation | accepted with the verdict read aloud; under 0.3 of time remaining | WF-06, WF-05, WF-07 |
| US-02 | Operator | P-1181 is denied (resource not ready); explain why without help | reason found in under 10 s | WF-06, WF-05 |
| US-03 | Operator | T-039 goes stale mid-decision | notices the stale label; does not accept; acknowledges the degraded picture or waits | WF-03, WF-07 |
| US-04 | Operator | At 02:03 the strip shows Detached | states what backend is live and what is queued; continues under delegation | WF-01 |
| US-05 | Operator | An alert storm (six raw alerts) arrives during a decision | completes the decision first; acknowledges the one incident afterwards | WF-08 |
| US-06 | Supervisor | The queue reaches seven with two expiring | reprioritises or delegates; no item lost; expiring ones handled first | WF-06, WF-17 |
| US-07 | Supervisor | Set the area layer to Hold for deconfliction | change made; sees which plans became denied | WF-01, FL-08 |
| US-08 | Supervisor | The KAL cell reconnects with one conflict | resolves it through PN-18 with the arbitration understood | WF-18 |
| US-09 | Sensor manager | R1 is lost at 01:50 | re-tasks R2 to search with coverage before and after shown; commits; reports the remaining gap | WF-10, WF-11 |
| US-10 | Sensor manager | A tasking request from Leyla costs defense coverage | tasks or declines with a reason; asks the supervisor's concurrence | WF-15 |
| US-11 | Analyst | Replay to 01:41:02 and find the decision on P-1183 | found within 2 min; annotation added | WF-12 |
| US-12 | Analyst | Generate the raid summary and trace MOE-04 to the journal | figure and reference found | WF-13 |
| US-13 | Administrator | A baseline with an invalid resource capacity | validation failure found at the field; apply stays disabled | WF-14 |
| US-14 | Intelligence analyst | Declare the ISR UAS hostile on emitter and behaviour evidence | declaration made above the margin; evidence retained | WF-04 |
| US-15 | Planner | Compare laydowns A and B and submit B with its rehearsal | submitted with the record; the maintenance-window gap named | WF-16, WF-11 |
| US-16 | Commander | Accept the upper Vell gap with a warning obligation and an expiry | accepted; the expiry not indefinite | WF-17 |

## 5. Reporting

- One report per round: per task, completion, latency, errors, TLX, confidence,
  quotes; per principle, whether the wireframes made it visible; findings ranked by
  severity (blocks the task, slows it, cosmetic) with the wireframe change proposed.
- Findings that need engineering go to the gap register; findings that change the
  design go to the wireframes and this plan's next revision.
- The values from round 1 become the MOP-37 targets proposal for the owner
  (deferred under D-16).

## 6. Record of the first walkthrough (heuristic, drafting agent, 2026-09-04)

Not a user test. The drafting agent walked the sixteen tasks against the wireframes
with the nine principles as heuristics. Findings and the changes made:

| Finding | Severity | Change |
|---|---|---|
| The recommendation panel originally carried the accept control | blocks principle 2 | moved to the decision dialog (WF-05, WF-07) |
| Time remaining was shown as a creation time | slows US-01, US-06 | replaced with a countdown everywhere (principle 9) |
| The operator layout listed the track table first | slows US-01 | queue first (WF-L1) |
| Raw alerts in the operator's alert panel | blocks US-05 | incidents view with raw alerts inside (WF-08) |
| Reconciliation could be dismissed | blocks US-08 | PN-18 modal that closes only by deciding (WF-18) |
| Sensor mode control allowed invalid transitions | slows US-09 | only `can_transition_to` modes offered; before/after gating the commit (WF-10) |
| Delegation dialog defaulted to no expiry | blocks principle 9 for US-16 | expiry defaults to the battle rhythm (WF-17) |
| The assistant panel had a "apply suggestion" control in an early sketch | blocks principle 7 | removed; only navigation controls remain (WF-19) |

Open after the walkthrough: US-03's degraded-picture acknowledgement wording; the
queue's capacity indicator needs a definition of "sustained decisions per minute";
whether the commander should see the operator's full queue or a summary by default.

## 7. Round 1 on the built panels (decided 2026-09-06, D-28)

The owner decided that round 1 runs against the rendered panels, one participant per
role, and that its results become the MOP-37 target proposal. The session script, the
task cards, the scoring sheet and the report template are in
[`usability-round-1-session.md`](usability-round-1-session.md). Three things are stated
there and repeated here because they bound what the round can measure:

**Re-planned 2026-09-06 and again 2026-09-08 (D-28, GAP-074): round 1 is fourteen
tasks, not ten.** Points 1 and 2 below record the split as D-28 first resolved it, and
it moved twice afterwards as engineering closed the gaps it rested on. GAP-050, GAP-057
and GAP-041/GAP-004 closed on 2026-09-07, which unblocked US-04, US-08 and US-09;
GAP-045 landed PN-16's rehearsal on 2026-09-08, which unblocked US-15. Round 1 is
therefore **fourteen of the sixteen**, and **US-07 and US-16 alone are round 2's** --
their blockers, an unwired control-status write and an undesigned gap-acceptance
control, are unchanged. The four late additions each need a session setup heavier than
the seed's own flag, and each needs its own dry run before the first participant; the
session document's §2 and §3 carry the current split and the setups, and are the version
to run from. Point 3 is unaffected by any of this and still governs.

**§4's task table is the wireframe-era definition and is not what a moderator reads.**
It is kept as written because it records what each task was meant to measure when the
round was designed, but the built panels have outrun some of it, and §5 of the session
document holds the cards actually read aloud. US-15 is the clearest case: §4 asks the
planner to "submit B with its rehearsal" and to have it "submitted with the record", and
PN-16 draws no adoption control at all -- deliberately, per DN-26 §6 rule 4, because
moving a sensor is a physical act with an authority chain this system does not model.
The session card was corrected on 2026-09-08 to ask the planner to say the control is
absent and why. §4's "laydowns A and B" moved the same day for a different reason:
PN-16 reads coverage from sensor placements alone, so the two round-1 laydowns then
declared -- which differ only in where a battery stands -- could not be told apart on
the table §4 has them compared on. The owner's answer was to give the round a pair that
differs in *siting*, and `round-1.json` now declares a third laydown `c`; the session
card compares `current` with it. Read §4 for intent; run from the session document.

1. **Ten of the sixteen tasks can run in round 1**, and four of those ten only once the
   live desktop can be seeded with tracks and plans. The tracker and the allocator both
   run as of 2026-09-06 (GAP-011, GAP-029), so the reason a round still needs a seed is
   not that the software cannot produce a picture but that a round needs the *same*
   picture for every participant, which no live sensor set on a test laptop provides; a
   seed that drives `ApprovalWorkflow` and
   `TrackingService` from a scenario file, stamped as a rehearsal in the journal, is
   **GAP-089**, which landed the same day, so all ten run.
2. **Six tasks are round 2's**: two need panels that are not built (PN-16, PN-18) and
   four need writes that do not reach the desktop (a verified node sign-in, the
   control-status change, the sensor command's transport, gap acceptance). Each is
   reported as not run with its gap, never scored.
3. **A target is proposed only from a measured value.** The report proposes, per
   measure, the round-1 median as the target and the worst case as the floor, with the
   sample size beside each; fewer than four sessions behind a measure makes it
   provisional, and a measure the round could not feed is "not measured".

What the owner supplies -- participants, dates, the consent process -- is listed in
the session document's §1. The session on 2026-09-05 that this plan records as begun
and incomplete is superseded by this round and is not counted toward it.

## Traceability

- Measures: MOP-37 (`../mission/capabilities/measures-catalogue.md`); MOE-04.
- Round 1: [`usability-round-1-session.md`](usability-round-1-session.md); D-28;
  GAP-074, GAP-089.
- Vignettes: VG-01, VG-07, VG-08, VG-09, VG-10.
- Plan 06 acceptance criteria; GAP-074 (run the sessions).

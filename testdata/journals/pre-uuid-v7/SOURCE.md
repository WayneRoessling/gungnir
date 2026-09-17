# A journal written before GAP-130

Written 2026-09-17 at commit `7d6f201` ("DN-31: a node approval queue, planned on D-55 to
D-59 (#127)") and committed on `51afa6e`, which changes documents only, before any
identifier type changed. It is for DN-31 §9 row 1: "a pre-change journal replays and
reports unchanged" (`docs/design/DN-31-node-approval-queue.md`, D-56, GAP-130).
**Every decision and plan identifier in it is one of the old `u64` counters**,
which restarted at 1 in every process: plans 1, 2 and 3 from `DpInterceptService`'s
counter, and decision 1 from `InMemoryApprovalWorkflow`'s. Nothing here is real and no
licence attaches.

## How it was written

By a temporary integration test in `gungnir-app`, marked `#[ignore]`, run once with
`cargo test -p gungnir-app --test pre_uuid_v7_fixture -- --ignored` and deleted before this
directory was committed. It was not kept because from GAP-130 on the same code mints UUID
v7 identifiers: a generator left beside this fixture would claim to reproduce a file it can
no longer write.

The test built a desktop (`AppState::with_config`) over a scratch data directory, with a
baseline carrying one point-layer resource (id 1, capacity 4) whose handoff endpoint is
`battery-2`, an endpoint of kind `handoff` (which no transport carries, so delivery is
recorded undelivered without any network), control status `free` for the point layer, an
authority rule letting the Operator and the Supervisor decide point-layer plans, a 90 s
expiry and 45 s escalation for the point layer, and a 30 s point-layer effect window.
Nobody was signed in, so the role was the desktop's default, Operator, and no decision
names an operator. The picture was a scripted tracking service, as
`gungnir-app/tests/engagements.rs` uses; the planner, the policy chain, the queue, the
engagement and handoff paths and the journal were the desktop's own. The clock was a
`ReplayClockAuthority` set before each `update::tick`.

| Mission time | What the test did | What the journal holds (sequence numbers) |
|---|---|---|
| 0 s | One hostile track, 7; tick | `PlanProposed` plan 1, `PlanEvaluated` plan 1 (0, 1); governance and health (2, 3) |
| 1 s | `decisions::decide` on the queued item, accepted; tick | `Decided` plan 1 decision 1 (4); `HandoffEvent::Issued` and `Undelivered` decision 1 (5, 6); `EngagementEvent::Opened` decision 1 plan 1 (7) |
| 2 s | Tick, with track 7 still in the picture | nothing new |
| 3 s | Published `HandoffEvent::Reported` (acknowledged) for decision 1, then called `handoffs::apply_report`; tick | `HandoffEvent::Reported` decision 1 (8) |
| 5 s | Emptied the picture; tick | `PlanProposed` plan 2, the empty plan, and `PlanEvaluated` plan 2 denied (9, 10); `EngagementEvent::Closed` decision 1, effective on track-lifecycle evidence (11) |
| 10 s | One hostile track, 8; tick | `PlanProposed` plan 3, `PlanEvaluated` plan 3 (12, 13) |
| 56 s | Tick | `CommandEvent::Escalated` plan 3 to the Supervisor (14) |
| 101 s | Tick; then `close_session` | `CommandEvent::Expired` plan 3 (15) |

**One line is not the desktop's own path.** Sequence 8 was published onto the desktop's bus
by the test. A linked desktop never journals it: the node records an effector's report
(`gungnir-node/src/main.rs`) and the desktop applies it from its link inbox through
`handoffs::apply_report`, which publishes nothing. The test called `apply_report` as well,
and put the node's line on this journal so that one session carries a report naming its
decision.

**What the journal does not carry.** No event names a queue item, so the two
`PendingApprovalId`s the queue minted (1 and 2) are not here. The expiry at 101 s also
minted decision 2 in the workflow's own record, and `CommandEvent::Expired` names the plan
only.

## Files

| File | What it is | SHA-256 |
|---|---|---|
| `session-000000000001.jsonl` | Session 1's journal, 16 envelopes, copied unchanged from the scratch data directory after `close_session` | `6e5e84625620c1c63aef66d5ee0e322acc850169cb450170a035b33c1cc88e42` |
| `report.json` | `gungnir_reporting::JournalReportGenerator::generate` over this directory's journal, written by `JournalReportGenerator::export` | `ec906815d8b0a1051e2a8d8374b863f4a5c012a1cb7579f3387e148f6eda7cc8` |

The report was generated from the copy in this directory, not from the scratch one, so it
is what regenerating from these files gives. Neither file is edited or regenerated: a
changed file is a different fixture, and the point of this one is that it was written by
the code as it stood before the change.

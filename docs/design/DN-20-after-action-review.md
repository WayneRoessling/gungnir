# DN-20 After-action review workflow

Closes GAP-049. Status: first draft, 2026-09-05. **Design only; no code exists.**

## 1. The gap and the thread step it blocks

Reports are generated and exported. Nothing holds a review session, its findings, or the
lessons that came out of it, so MOE-12 (rehearsal effect) cannot be tracked across events
and a lesson lives in whoever attended.

## 2. The owning component

`gungnir-workflow`, which already owns `Case` and `Annotation`. A review is a case with a
session and a report attached.

## 3. Types

In `gungnir-workflow`:

```rust
/// A review of one session, with what was found and what was decided about it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReviewCase {
    pub case: Case,
    pub session: SessionId,
    /// The report the review was conducted against, so a finding can be checked.
    pub report: Option<ReportId>,
    pub findings: Vec<Finding>,
    pub state: ReviewState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReviewState {
    Open,
    /// Findings recorded, actions assigned.
    Concluded,
    /// Every action closed.
    Closed,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    pub id: FindingId,
    pub summary: String,
    pub kind: FindingKind,
    /// The moment in the session it refers to, so the reviewer can replay to it.
    pub at: Option<MissionTime>,
    /// Tracks, decisions, or alerts the finding is about.
    pub refers_to: Vec<FindingSubject>,
    pub action: Option<FindingAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FindingKind {
    /// Something the system did wrong or failed to do.
    SystemBehaviour,
    /// Something a procedure did not cover.
    Procedure,
    /// Something a person did that is worth repeating or not repeating.
    Practice,
    /// A configuration that turned out wrong.
    Configuration,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FindingAction {
    pub owner: String,
    pub due: Option<MissionTime>,
    pub closed: Option<MissionTime>,
    pub outcome: Option<String>,
}
```

## 4. Edges

**None.** `gungnir-workflow` already depends on `gungnir-model`, `gungnir-security`, and
`gungnir-observability`. `SessionId` and `ReportId` are model identifiers; the review does
not open a report itself, it names one, and the panel fetches it. That keeps workflow out
of `gungnir-reporting`.

## 5. Behaviour

**A finding points at the record, not at a memory.** `at` and `refers_to` let the panel
seek the replay to the moment and select the track or decision in question. A finding that
says "the queue got behind around eleven" and cannot be seeked to is an anecdote.

**Findings are typed, and the type matters.** A `SystemBehaviour` finding is a candidate
gap-register entry; a `Configuration` finding is a candidate baseline change; a `Practice`
finding is neither and must not be filed as a defect. Separating them stops the review
producing a list of engineering tickets for what were actually training points.

**A review concludes; actions close separately.** `Concluded` means the findings are
recorded and owned. `Closed` means every action is done. Conflating them would let a review
be declared finished with its actions open, which is how lessons stop being learned.

**Nothing is automatic.** The system does not propose findings, score the session, or
judge the operators. It assembles the record, seeks to the moment, and holds what people
concluded. MOE-12 counts reviews and their findings; it does not grade them.

**Link to the gap register.** A `SystemBehaviour` finding may be promoted to a gap. The
design records the gap identifier on the finding when that happens, so the loop from an
observed failure to a tracked engineering item is visible. It does not file the gap
automatically, because the register's entries need scoring and an owner.

## 6. Configuration and interface delta

None in the baseline. Review is a workflow, not a policy.

Interface, additive:

| Method and path | Purpose | Authorization action |
|---|---|---|
| `POST /v2/reviews` | Open a review against a session | new action `review.conduct` |
| `POST /v2/reviews/{id}/findings` | Record a finding | `review.conduct` |
| `POST /v2/reviews/{id}/state` | Conclude or close | `review.conduct` |
| `GET /v2/reviews` | List, filtered by DN-17 | `picture.view` |

`Event` gains `Review(ReviewEvent)` with `Opened`, `FindingRecorded`, `Concluded`,
`Closed`, so a review leaves a journal trail like everything else.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-12 Replay | Findings shown on the timeline; recording a finding at the current position is one control |
| PN-13 Reports | The review case beside the report it was conducted against |
| PN-08 Alerts | Findings with an open action past their due date |
| PN-17 Commander summary | Open reviews and open actions |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-5.3 Reports and measures, review part | Workflow unit tests plus a replay-and-review pass | A review cannot be closed with an open action; every finding with an `at` seeks the replay to that mission time; findings carry their kind and are counted separately by kind; a promoted finding records the gap identifier; every state change is journalled with an operator | TT-01 replayed and reviewed end to end |

## Traceability

GAP-049; CAP-5.3; MOE-12; depends on GAP-047 for the measures the review reads;
`../ux/wireframes/WF-12-replay.puml`, `WF-13-reports.puml`;
`../mission/gap-analysis/README.md` for the register a finding may be promoted into;
principles AP-01, AP-03, AP-08.

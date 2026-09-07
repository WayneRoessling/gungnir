# Task analysis: Analyst (P-03)

Threads: MT-09 (after-action review, measures, model governance). Layout: PN-12,
PN-03, PN-13 beside the viewport; PN-04, PN-19 on demand.

## T-an-1 Replay a session (OA-25, OA-26)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 1.1 Open a recorded session | session list from the journal (`SessionId`), length, first and last mission time | none | opening the wrong shift | H | PN-12 |
| 1.2 Scrub to a moment | `ReplaySession::seek_to`, position and remaining, the replay clock | none | the picture at the scrubbed time not matching the live session (MOP-15) | H | PN-12, PN-02 |
| 1.3 Step through events | `ReplaySession::step`, the envelope's `seq`, `mission_time`, event kind | none | losing the place; gaps in `seq` unseen | H | PN-12 |
| 1.4 Inspect a track or decision at that moment | `TrackView` as of the envelope; `DecisionRecord` | none | mixing replayed and live state | H | PN-04, PN-03 |
| 1.5 Annotate | `Annotation` with author, time, text, track (`Case`) | analyst | annotation lost with the session | H | PN-12 |

## T-an-2 Produce reports and measures (OA-11, OA-26)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 2.1 Generate a report for a session | `Report` with figures and the journal references behind each | none | a figure without a reference | B | PN-13 |
| 2.2 Compute the measures | MOE and MOP values per the catalogue (GAP-047) | none | measures estimated by hand | B | PN-13 |
| 2.3 Export | export package with provenance | analyst | exporting without marking (D-06) | B | PN-13 |

## T-an-3 Govern models (OA-33)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 3.1 Review a candidate baseline's validation evidence | model registry entry, validation status, comparison against the baseline in force | none | promoting without evidence (refused by SV-19) | H | PN-14 |
| 3.2 Promote with concurrence | promotion with the supervisor's concurrence recorded | analyst | promotion during a live session | H | PN-14 (dialog) |
| 3.3 Roll back | previous baseline restored | analyst | rollback without a reason | H | PN-14 |

## T-an-4 Record lessons (OA-26)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 4.1 Draft findings from the replay | annotations, measures, the assistant's draft with provenance (plan 08) | analyst | assistant text taken as fact | B | PN-13, PN-19 |
| 4.2 File into the gap register | the finding as a gap or evidence | analyst | findings in a document nobody reads | B | outside the product |

## Error modes that shape the design

- Replay and live are never on the same screen: the strip says Replaying and the
  viewport background carries a replay watermark.
- Every report figure is a link to the journal envelopes that produced it.
- Model promotion is a dialog that shows the validation status and refuses without
  it (SV-19 semantics), so the UI cannot ask for something the crate refuses.

## Traceability

- Activities OA-11, OA-25, OA-26, OA-33; capabilities CAP-5.2, CAP-5.3, CAP-5.7;
  wireframes WF-12, WF-13, WF-14, WF-19; flows FL-03, FL-06.

# Effector reports reach the record

GAP-135 closes. An effector's report that moved an engagement changed it in memory and put
nothing on the record. `ApprovalDesk::apply_report` (`gungnir-approval/src/handoffs.rs`)
called `Engagement::executing` for an `Executing` report and `close_effective` or
`close_ineffective` for a `Completed` one, and published no `EngagementEvent` for either.
Only the engagement sweep and the abort published closes.

**What that cost.** No journal of a real session could hold `Executing`, or a close on an
effector's word. The report's `engagements_effective_corroborated` and
`engagements_ineffective_corroborated` counts could never be non-zero, so PN-13 showed
every effective engagement as track-inferred or not at all. A replay showed the
engagement open until its effect window closed. The after-action account read
track-lifecycle evidence where an effector had reported. GAP-130's build found it: its
pre-change journal fixture had to close its engagement on track-lifecycle evidence,
because a close an effector reported never reached a journal.

**The fix.** When the engagement takes a report, the move goes on the record: an
`Executing` report publishes `EngagementEvent::Executing`, and a `Completed` report
publishes `EngagementEvent::Closed` with `effective (effector or operator evidence)` or
`ineffective (effector or operator evidence)`. The close goes through
`engagements::publish_closed`, now crate-visible, which is the one place closes are
published, as the gap asked. `close_engagement` returns the corroborated label only when
the engagement actually closed, so **a report the engagement refuses publishes nothing**:
a second, contradictory completion against a closed engagement is still said in the alert
and kept on the handoff's report record for PN-20, and never reaches the journal as a
close that did not happen.

**Which time the events carry.** Both carry the time this host received the report, the
same time the sweep's closes carry. The effector's own times are already on the record
where they belong: in the handoff's report list, and in the close's evidence
(`EffectSource::EffectorReport`, `observed_at`).

**Test.** `an_effector_completion_reaches_the_journal_as_a_corroborated_outcome`
(`gungnir-app/tests/handoffs.rs`) opens an engagement and applies `Executing`, an
effective `Completed`, then a contradictory second `Completed`. Exactly two engagement
events are published, `Executing` and `Closed` as effective corroborated, and none for the
refused third report. After a tick, the report PN-13 generates from the desktop's own
journal counts one effective corroborated engagement and no track-inferred one. With the
`gungnir-approval` change reverted, the test fails.

**Human-owned.** `gungnir-approval` is the decision path, human-owned wherever it runs
(D-65), so this came to the owner as a review.

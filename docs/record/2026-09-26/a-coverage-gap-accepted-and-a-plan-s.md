# A coverage gap accepted, and a plan's laydown rehearsed or said

GAP-106 and GAP-107, filed by D-50, and GAP-193 to GAP-195 found building them
([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml));
D-118 to D-120 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml));
[`../../design/DN-33-accepting-a-coverage-gap.md`](../../design/DN-33-accepting-a-coverage-gap.md),
new, and [`../../design/DN-26-laydown-options.md`](../../design/DN-26-laydown-options.md)
§11, amendment 1. Designed and built on 2026-09-26. The owner decided both questions that
day, directly; the details were taken under the owner's delegation of the same day.

## What was missing

**GAP-106.** DN-12 found the holes along the approaches and PN-11 drew them, but nothing
let anyone say a hole was known and tolerated. A gap a commander had argued about and
taken on read exactly like one nobody had seen. §4 of the roles document gave the
decision to the commander and no code could take it, so US-16 was held for round 2.

**GAP-107.** DN-26 §6 rule 4 built no adoption step, so there was nothing for a rehearsal
to gate, and a plan proceeded exactly as it would have without one. The record could not
say whether the laydown under a decision had ever been tried.

## What the owner decided

**D-118 (GAP-106).** An acceptance names the gap, a reason (required) and the commander
who accepted it. It holds for the baseline revision and the laydown it was made under,
and re-opens by itself if either changes or the gap's own shape changes. Accepted gaps
stay drawn on PN-11 and PN-16, marked accepted, and still count in the coverage measure,
which reports accepted and unaccepted separately rather than subtracting. Only the
commander may accept. Accepting and re-opening are events on the record, journaled and
audited.

**D-119 (GAP-107).** Advisory and acknowledged, with no hard gate. PN-16 and the plan
path say plainly when the laydown in force was never rehearsed, or was rehearsed under an
older baseline revision or policy, and a decision asks the operator to acknowledge that.
The acknowledgement is audited and travels with the decision record.

## What was decided under the delegation, and why

**What "the gap's own shape changes" means.** The same approach by name, the same
severity, the same sample spacing and masking flag, and each end within half a sample
spacing. The sampler puts every sample at a multiple of the spacing along the whole
polyline (GAP-118), and a gap's ends are samples. So a real change moves an end by at
least one spacing, and recomputation moves it by rounding only. Half a spacing is the
widest tolerance that cannot swallow a one-sample change. The unit test holds it to one
sample either side. Rejected: an overlap or containment rule under which a shrunken gap
still holds. It reads kindly, but the commander accepted a particular risk, not a
ceiling on one, and a smaller gap is a different risk. Also rejected: a tolerance in
metres, which would mean different things at different spacings.

**Who "the commander who accepted it" is.** A signed-in commander. An acceptance is
refused with nobody signed in, even when the selected role is the commander. A decision
may be recorded unattributed because its missing name is itself on the record, but an
acceptance exists to carry a name. The administrator does not hold `coverage.accept_gap`
either. §4's "Roles without a column" gave the administrator every decision but the
engagement chain and escrow. The owner's "only the commander" put risk acceptance with
the commander, and §4 now says the administrator is withheld from it too, for the reason
§1 gives about the engagement chain.

**When re-opening is checked.** Every frame while any acceptance stands, against the live
report PN-11 draws. The check waits while terrain the baseline names is still loading.
Without that, every restart would compute a flat report before the mask arrived and
re-open every acceptance, and a terrain that fails is then a shape change like any
other. **Found building it**: a desktop starts with every sensor at standby, so the
first frame after a restart sees the whole approach uncovered. That re-opens every
acceptance unless the radars are commanded before it. That is the owner's rule working as
written, because the gap on the approach really is a different gap, so it is said on
PN-11 with the reason rather than papered over. US-16's session setup (the session
document §3) commands the radars first.

**What "rehearsed under an older revision or policy" means (D-119).** A rehearsal records
what it ran under as five SHA-256 digests of the configuration the throwaway desktop is
built from: placements, sensors, resources, policy and tracking. It is computed by
calling the same `config_for` the run calls, so it cannot drift from D-113's list. A
rehearsal of the laydown in force stands when any rehearsal of it on the record ran under
the five parts now running. A new revision that changed none of them (an endpoint edited,
an approach renamed) does not stale it. PN-16 still names the revision, and nothing is
asked, because asking about a change that cannot move a rehearsal's answer teaches people
to tick the box unread. When parts differ, the sentence names them and both revisions.
Rejected: comparing revisions alone, which asks on every edit, and comparing the policy
alone, which misses a radar moved in the laydown or a tracker re-tuned.

**Rehearsals on the record (D-120).** Rehearsal figures were session state, so every
restart would have made the laydown in force "never rehearsed", a false statement put on
a decision record. A rehearsal now journals a `RehearsalStamp`, and the desktop folds the
stamps at start. Retention keeps each laydown's latest, just as it keeps a standing
acceptance and the session with the highest acceptance number. PN-16 says a laydown was
rehearsed in an earlier session and does not invent that session's figures.

**What the decision carries.** PN-07 draws the sentence in its own section, not among the
degraded conditions, because nothing is failing. Accept and override stay disabled until
that sentence is ticked, and the tick is reset when the sentence changes. A rejection asks
nothing. `DecisionRecord::acknowledged` and `CommandEvent::Decided::acknowledged` carry
the sentence, and the decision's single audit entry quotes it. `decisions::decide` refuses
an actionable decision without the tick (`CommandError::Unacknowledged`) and records
nothing, so the rule holds beyond the button. A deployment that declares no laydown is
told and asked nothing, because there is nothing to rehearse. An unanswerable question
on every decision would be the click-through this avoids.

## What was built

- `gungnir-model`: `GapSeverity` moved here and re-exported by `gungnir-analytics`;
  `AcceptedGap`, `GapAcceptance`, `ReopenedBecause`, `RehearsalBasis`, `RehearsalStamp`,
  `Acknowledgement`; `PlanningEvent` on `Event::Planning`, additive;
  `CommandEvent::Decided::acknowledged`, defaulted and left out when empty.
- `gungnir-analytics::coverage`: `accepted_gap`, `same_gap`, `standing`,
  `CoverageMeasure`.
- `gungnir-security`: `coverage.accept_gap`, the commander's alone, with §4 and
  `role_matrix.rs` changed first.
- `gungnir-command`: `DecidedBy::acknowledged`, `DecisionRecord::acknowledged`,
  `CommandError::Unacknowledged`. `gungnir-approval`: `decide_acknowledging`, and the
  acknowledgement on the decision's audit entry.
- `gungnir-app`: `gap_acceptance`, `rehearsal_standing` and `planning_record`, the
  re-open check in the tick, retention's protections, `laydown_rehearsal::basis_of`, the
  gate in `decisions::decide`, and the same question before a linked desktop posts.
- `gungnir-ui` and `gungnir-viewport3d`: PN-11's gap list and accept control, the gap
  layer's "accepted" mark, PN-16's accepted cell, its in-force sentence and its
  earlier-session rehearsal, PN-07's rehearsal section, and the strip's accepted count.
- `gungnir-reporting`: the watch report counts acceptances, re-openings and rehearsals.
- US-16 moved into round 1 with a card for what was built (the session document §2, §3
  and §5).

## Human-owned

`gungnir-security` (the action and its grant) and `gungnir-command` (the record's new
field and error) are human-owned; what the owner has reviewed is in
[`../../signatures.md`](../../signatures.md).

## What this does not do, and where it went

- **GAP-193.** A desktop linked to a node asks and gates the same way and audits the
  acknowledgement on its own log. The node's decision record does not carry it, because
  the `/v3` decision route has no field for it. A cut-off desktop's forwarded decision
  loses it at the node for the same reason. Carrying it is a change to a `gungnir-api`
  write path, which is human-owned.
- **GAP-194.** An acceptance is held by the desktop that recorded it. A node's coverage
  answer and a second desktop's PN-11 do not show it.
- **GAP-195.** PN-07's degraded-condition acknowledgement gates accept but is on no
  record. Only the rehearsal acknowledgement travels with the decision.

## Verification

`gungnir-app/tests/gap_acceptance.rs` and `gungnir-app/tests/rehearsal_advisory.rs` run
over round 1's committed baseline and laydowns, and the second also over TT-11, the
committed recording of round 1's raid. The unit tests are in `gungnir-analytics`
(the tolerance, the order of the checks, the measure), `gungnir-app`'s
`laydown_rehearsal` (the basis changes with every setting a rehearsal takes and with
nothing else), `gungnir-ui`'s decision dialog (the gate) and `gungnir-security` (the
grant). Both rows are Draft rows of the verification table, not agreed criteria, and
walking them is the owner's.

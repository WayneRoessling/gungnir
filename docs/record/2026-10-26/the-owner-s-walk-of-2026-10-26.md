# The owner's walk of 2026-10-26

Plan 12 (`../../plans/12-review-closure.md`) raised nine decisions, D-136 to D-144, out
of the comprehensive review of the same date (`../../review/2026-10-26-comprehensive-review.md`).
The owner walked them the same day, four at a time, each with a recommended default.
Seven were taken to the default and two past it. Nothing is built by this change; each
decision's outcome is in `../../mission/gap-analysis/data/decisions.yaml`, and each gap a
decision moves carries a history entry dated today.

| Decision | What the owner chose | Against the default? |
|---|---|---|
| D-136 | Widen `Classification` to STANAG 1241's set; `Unknown` stays the default; Pending, Assumed Friend and Suspect weighted; Joker and Faker carried, never weighted | No |
| D-137 | NFFI joins ASTERIX Category 062 as a bearer in this release; Link 16 J-series over JREAP-C and VMF out of scope and recorded as such | **Yes**: the default left NFFI out |
| D-138 | Vertical scaling, one node per sector, an active/standby pair through journal shipping, designed before built | No |
| D-139 | The full MIL-STD-2525D symbol set; echelon and unit modifiers blank until a unit model exists | **Yes**: the default was frames and affiliation only |
| D-140 | Single approval recorded as the rule; an optional two-person rule for override and control status as a baseline setting, off by default | No |
| D-141 | Engagement opened and closed published over the exchange by global identity; sector ownership deferred | No |
| D-142 | A deployment `AuditSigning` purpose provisioned with the baseline, sealed under the deployment key the operator session never unlocks; anchors forwarded to the node when linked | No |
| D-143 | Twelve-hour maximum session, fifteen-minute idle lock, both configurable downward only; a locked screen keeps the picture and queue visible and refuses every action | No |
| D-144 | A fixed-site counter-UAS trial with SAPIENT sensors and one effector; GAP-207's adapter built against the SAPIENT task path until a partner names its interface | No |

## What the two departures changed

- **D-137 filed two gaps.** GAP-241 (ASTERIX Category 062 encode for system tracks) and
  GAP-242 (the NFFI bearer, inbound and outbound) join GAP-090 and GAP-091 in tranche C,
  so the tranche carries three bearers rather than one. The owner's reasoning: NFFI
  (STANAG 5527, ADatP-36) needs no terminal, and a partner that does not run TAK still
  has to be able to report its positions; the blue-force feed is worth the cost of
  pinning a specification the repository does not yet hold. Both new gaps depend on
  GAP-206 (tracks releasable) and GAP-216 (the widened identity), and
  `../../design/external-standards.md` gains the NFFI pinning before either is built.
- **D-139 kept GAP-227's effort at L** but changed its shape: one pass over the glyph
  code that draws the whole 2525D set, with echelon and unit modifiers rendered blank,
  rather than a frames-only pass now and a second one later. The amplifiers the picture
  already carries (speed, heading, altitude, identity, quality) draw from the first build.

## What the walk did not decide

- Which radar, RF detector and effector the trial uses: D-144 names none until a partner
  does.
- Sector ownership between nodes: D-141 deferred it until a unit model and an airspace
  control order import exist.
- The exact `AuditSigning` provisioning flow and how a deployment key reaches a desktop
  that was never linked: GAP-208's design, under DN-22's custody profiles.
- Whether `docs/safety/` is the right home for the hazard analysis and the security plan:
  plan 12's open question, not a decision.

## Facts the questions rested on

- Every evidence line in the nine questions was re-checked against the code at
  `9e9fe9f5` during the review and again before the plan was written; the walk changed no
  evidence, only what to do about it.
- The register's increments do not drive the tranche order: GAP-206 is I3 and sits in
  tranche A because the two-node picture is the first thing a trial partner will ask to
  see, and because every bearer in tranche C depends on it.
- The nine decisions were all raised and taken within one branch (pull request #228), so
  none was ever merged open; the generator's append-only check on `origin/main` sees nine
  new resolved entries, not nine edits.

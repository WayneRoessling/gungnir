# A note on §10's "Resolved on 2026-09-06" heading

This paragraph opened that heading in `ARCHITECTURE.md` §10, and was moved here on
2026-09-16 with its wording unchanged.

**Heading added 2026-09-07.** Everything from here to item 101 was already dated
2026-09-06 or 2026-09-07 and already described completed, signed work; none of it
was open. It had no heading of its own and sat under `### Open` above by omission,
not by finding, for the time between whenever each item landed and this correction.

- **Plan 10 architecture governance (2026-09-04; signed 2026-09-05).** Seventeen
  architecture principles and seventeen contracts in
  `docs/architecture/togaf/preliminary/architecture-principles.md` and
  `docs/architecture/togaf/phase-g-implementation-governance/architecture-contracts.md`
  were signed by the owner on 2026-09-05, in four batches, each checked against the code
  first. Five findings are recorded at signature rather than resolved before it: C-01 and
  C-04 have no automated check (GAP-039, GAP-059); AP-08 is signed knowing D-04's
  buffering bounds desktop durability at 5 s (item 32); AP-12 is signed as an intended
  rule with 22 `todo!()` calls outstanding and GAP-082 open; AP-11 carries two accepted
  exceptions, `solve_assignment` and `compute_metrics` being free functions rather than
  traits; and **AP-16's merge gate has no mechanism at all**, because the workspace is
  not under version control and the CI workflows have never run. C-07 and C-11 were run
  against the tree on the day and both passed. The first compliance assessment passed six
  checks and raised two findings, both about the checks not being automatic rather than
  about the code. A DoDAF cross-reference (`docs/architecture/togaf/framework-cross-reference.md`)
  answers plan 10's open question about external frameworks; no customer framework is
  committed.

- **Plan 11 design closure (2026-09-05).** `docs/design/` holds a design note for every
  gap where the architecture did not name a component responsible: 22 notes and 4
  consolidations. Design coverage across the 56 mission capabilities moved from 30 full to
  52; 23 gaps were retyped from mission to technical because they are now designed and
  awaiting implementation, and 3 mission gaps remain, all owned by plans 07, 08, and 09.
  Two things in that set need this document changed before any of it is built, and neither
  has been:
  - **Five new dependency edges**, listed with their justification and an acyclicity check
    in `docs/design/dependency-edges.md`: `gungnir-workflow` to `gungnir-assessment` and to
    `gungnir-sensor-management`, `gungnir-analytics` to `gungnir-sensor-management`,
    `gungnir-decision` to `gungnir-analytics`, and `gungnir-reporting` to
    `gungnir-identity`. Each is drawn in §7.1 in the change that adds it to a manifest,
    never before. Four further edges were refused, one of them impossible under the
    one-way rule: a service facade may not depend on a productization crate, so engagement
    state keys on a new `DecisionId` in `gungnir-model` instead.
  - **One breaking change to the canonical model, decided 2026-09-05**:
    `PlanView.solutions` becomes `PlanView.kind: PlanKind` so a plan can be an intercept or
    a fires task. That changes a field's type, which the contract's own compatibility rules
    say needs a new schema version and a new path version. The owner took option B:
    `gungnir_model::SCHEMA_VERSION` goes from 1 to 2, the path goes from `/v1` to `/v2`,
    and `solutions` is removed rather than kept as a deprecated mirror. Removing `/v1`
    meets the contract's own condition rather than excepting it, because no client is
    deployed against it. **Landed 2026-09-05**: `gungnir_model::SCHEMA_VERSION` is 2, the
    interface module is `gungnir-api/src/v2/`, and ten call sites across six crates moved
    to `PlanView::solutions()`. `docs/gungnir-api-v1.md` (which keeps its filename so the
    doc-comment citations stay correct) and `docs/design/model-and-schema-deltas.md` §3
    carry the decision.
  - **Signed off 2026-09-05**: all five human-owned notes, DN-08, DN-09, DN-10, DN-17,
    and DN-22, and all 23 verification rows, which are now agreed criteria in
    `docs/verification-capability-table.md` §2.
  - **Implemented 2026-09-05**: all twenty-two notes. The workspace carries 340 passing
    tests, and every safety rule the notes name has a test behind it. Three type
    placements moved during implementation, each recorded in the note that assumed
    otherwise: the asset list is anchored to the local frame by the caller (DN-01 §3a),
    the anomaly detectors take primitive snapshots rather than model types (DN-15 §3a),
    and `SessionId` moved from `gungnir-store` down to `gungnir-model` because six
    crates share it. None of the three added an unapproved edge, which is what they were
    avoiding.
  - **Reviewed 2026-09-05**: the engineering reviewer accepted all five dependency edges,
    including `gungnir-analytics` to `gungnir-sensor-management`, the one the set flagged
    as weakest; and a domain reviewer checked each note against the mission thread step it
    names. The design set is fully signed and reviewed, and every note is cleared to
    implement. Each edge is drawn in §7.1 in the change that adds it to a manifest.

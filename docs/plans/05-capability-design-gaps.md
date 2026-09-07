# Plan 05: Mission and technical capability design gaps

## Purpose

Produce a single, prioritized register of the gaps between what the missions need
(plan 04) and what Gungnir is designed to provide, split into mission gaps (a needed
capability the design does not cover or covers partly) and technical gaps (a designed
capability whose implementation, verification, or integration is incomplete), each
with a closing action, an owner, and a target increment. Technical gaps that are
already tracked in `ARCHITECTURE.md` §10 are referenced, not duplicated.

## Scope

In scope: coverage assessment per capability, the gap register, severity and priority
scoring, closing actions, and the roadmap of closures. Out of scope: performing the
closures.

## Inputs

- `docs/mission/capabilities/` (plan 04), especially the two matrices.
- `ARCHITECTURE.md` §10 (open items), `docs/verification-capability-table.md` §2
  (draft rows), `docs/gungnir-capabilities.md` §9 (principal risks),
  `docs/performance-budgets.md` (unmeasured budgets).
- `docs/mission/measures.md` for what "covered" means quantitatively.

## Deliverables and target location

All under `docs/mission/gap-analysis/`:

| File | Content |
|---|---|
| `README.md` | Method, scoring scheme, how the register is maintained |
| `gap-register.md` | The register (one row per gap) with identifier, type, capability, description, severity, mission impact, affected crates or documents, closing action, target increment, owner, status |
| `coverage-matrix.md` | Capability versus design coverage (full, partial, none) with the evidence column |
| `technical-gap-map.md` | The technical gaps grouped by layer, each pointing at its `ARCHITECTURE.md` §10 item or verification-table row |
| `closure-roadmap.md` | Gaps by target increment, ordered by priority, with dependencies |
| `decisions-needed.md` | Gaps whose closure needs a scoping decision rather than engineering |

## Scoring scheme

- **Severity** (1 to 5): consequence for the mission thread if the gap remains
  (5: the thread cannot be executed; 3: degraded with workaround; 1: cosmetic).
- **Reach**: how many threads and roles the gap touches.
- **Effort** (S, M, L, XL): from the engineering estimate.
- **Priority** = severity times reach, adjusted by effort in the closure roadmap.

## Gap register template

```
GAP-xxx  <Short name>
Type:        Mission | Technical | Decision
Capability:  CAP-xx.yy
Description: What is needed and what the design provides today.
Evidence:    Document section or code path that shows the current state.
Severity:    1..5     Reach: threads and roles     Effort: S/M/L/XL
Impact:      The thread step that fails or degrades.
Closing action: What changes (crate, document, decision), in one or two sentences.
Target:      Increment n
Owner:       Role or person
Status:      Open | Planned | In progress | Closed (date)
```

## Method

1. **Coverage.** For each leaf capability, assess design coverage against the
   capability-to-crate matrix and the crate status; record evidence.
2. **Mission gaps.** Where coverage is partial or none, write a gap; check whether
   it is a scoping decision (for `decisions-needed.md`) or an engineering item.
3. **Technical gaps.** Import `ARCHITECTURE.md` §10 open items and the
   verification-table draft rows as technical gaps with references; add any
   integration gaps the coverage pass exposes (for example a capability provided by
   two crates that are not wired together).
4. **Score and prioritize** with the owner.
5. **Roadmap.** Place closures against increments; check dependencies; hand the
   engineering items to `ARCHITECTURE.md` §10 if they are not already there, so the
   open-items ledger stays complete.
6. **Publish and maintain.** The register is updated whenever a gap closes; the
   closure date is recorded.

## Roles

- Owner: severity and priority calls, scoping decisions.
- Analysis agent: coverage pass, register drafting, roadmap.
- Engineering reviewer: technical gap accuracy and effort sizes.

## Dependencies

Plan 04. Feeds plans 01, 03 (St-Rm), and 10 (phases E and F).

## Effort and sequencing

4 to 6 agent-assisted days; 1 week elapsed after plan 04.

## Acceptance criteria

- Every capability with partial or no coverage has a gap entry with evidence.
- Every `ARCHITECTURE.md` §10 open item appears as a technical gap with a
  reference, and every engineering closing action in the register appears in §10.
- Every gap has a severity, an owner, and a target increment or a decision entry.
- The closure roadmap has no dependency cycles and matches the increments.

## Risks

- Double bookkeeping between the register and `ARCHITECTURE.md` §10; mitigate by
  making the register reference §10 items by number and by a review step that
  reconciles both before publication.
- Severity inflation; mitigate with the thread-step impact statement per gap.

## Open questions

- Whether to keep the register as Markdown or move it to an issue tracker once the
  repository is hosted; the Markdown form is the source until then.

# Implementation and migration plan

Status: first draft, 2026-09-04. Phase F. The order the work packages are built in, what
each depends on, who does it, and what could stop it.

"Migration" here does not mean replacing a legacy system. There is no incumbent to
migrate from: the plan moves the architecture from a verifiable scaffold to a fielded
product across four increments.

## 1. The order

Full ordering with priority and effort per gap is
[`../../../mission/gap-analysis/closure-roadmap.md`](../../../mission/gap-analysis/closure-roadmap.md),
generated with a cycle check and an increment-ordering check. Summarized at the
work-package level:

| Step | Package | Depends on | Gate to pass before the next step |
|---|---|---|---|
| 1 | WP-05 build and release pipeline | Hosting decision D-10 | The gates actually run on hosted runners |
| 2 | WP-18 compliance automation | WP-05 | The contract checks fail the build when violated |
| 3 | WP-01 tracking pipeline and oracles | Nothing | Oracle agreement within the table's tolerance |
| 4 | WP-03 harnesses and test data | WP-01 for the generator | Budgets measured, not asserted |
| 5 | WP-02 the live data path | WP-01, decisions D-02, D-08, D-09 | Provenance traceable from a real wire to a view |
| 6 | WP-04 identity and session foundations | Nothing | |
| 7 | WP-07 the decision loop | WP-01 | A recommendation with a rationale, from a real picture |
| 8 | WP-08 authority and policy | WP-07, decisions D-05, D-15 | No execution without a recorded decision, tested |
| 9 | WP-09 role workspaces and panels | WP-08, decisions D-12, D-17 | A role can complete its thread in the interface |
| 10 | WP-06, WP-10, WP-11, WP-12 | WP-07 and WP-08 as each requires | Second-section verification rows promoted to gates |
| 11 | WP-13 transport | The stack sign-off for the transport crates | A desktop connects to a node and survives losing it |
| 12 | WP-14, WP-15, WP-16, WP-17 | WP-13 | Release evidence package complete |

Steps 1 and 2 come first for a reason that is easy to get wrong: **verification
infrastructure before the code it verifies.** Building the pipeline before the harness
means the pipeline's correctness rests on inspection.

## 2. Resources

One owner directing agents, with reviewers on call. Ownership per package is in
[`../phase-e-opportunities-solutions/work-packages.md`](../phase-e-opportunities-solutions/work-packages.md)
and derives from the gap register's owner column, where five
owner categories appear: tracking engineer and security engineer (both human-owned
crates), services engineer, user-interface engineer, data engineer, and the owner.

The human-owned share is the binding constraint, not the total effort. Four of the
seventeen work packages are entirely human-owned crates, and they include the two on the
critical path.

## 3. Dependencies outside the code

| Dependency | Blocks | State |
|---|---|---|
| Hosting with runners, including a graphics runner | Every gate, WP-05 | Decided (D-10), not stood up |
| Stack sign-off for the transport crates | WP-13 and everything behind it | Pending |
| Stack sign-off for the inference runtime | WP-17 | Pending |
| Stack sign-off for a docking crate | Part of WP-09 | Pending |
| Interface agreements with real external parties | The real versions of WP-14 | D-08 made endpoints generic so the work is not blocked; the agreements are per deployment |
| Subject-matter review of the mission content | Confidence in every package, not their execution | Not started |
| Usability sessions with participants | WP-09's targets | Not started |

## 4. Risks to delivery

| Risk | Effect | Mitigation |
|---|---|---|
| The tracking mathematics takes longer than any other single item | Increments 2 and 3 both slip | It is first, it is human-owned, and the oracles are specified before the code |
| One person is the board, the architect, and the reviewer | Quality and pace both depend on one person's availability | Structural gates rather than review headcount; the first hire is named in the business plan |
| A green core build is read as readiness | A capability is claimed that has not been demonstrated | GAP-067 promotes the second-section rows to gates in increment 3 |
| The scope lock is not revisited when it should be | Everything ships late rather than something shipping on time | D-B2 asks the question explicitly in the business plan |
| The assistant and the models expand scope quietly | Effort leaves the critical path | Both are separate packages, both are the named cuts if increment 3 slips |
| Budgets are never measured | Performance claims stay claims | WP-03 is step 4, before the decision loop, not after |

## 5. What is deliberately not planned here

No dates. The business plan carries the commercial schedule with its own confidence
marks, and the roadmap there sits on top of these increments. Repeating dates in a
migration plan would create a second schedule that disagrees with the first within a
month.

No resource loading in person-days per package either. The plan set carries effort ranges
per plan and the register carries effort per gap; multiplying them into a schedule would
imply a precision that neither input has.

## Traceability

`../phase-e-opportunities-solutions/work-packages.md` for the packages;
`../../../mission/gap-analysis/closure-roadmap.md` for the gap-level order;
`../phase-e-opportunities-solutions/transition-architectures.md` for the state at each
boundary;
`../../../business/roadmap.md` and `risk-register.md` for the commercial view;
`../../../plans/README.md` for the plan sequencing.

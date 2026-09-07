# Statement of architecture work

Status: first draft, 2026-09-04. Phase A. The scope, schedule, roles, and acceptance for
the architecture work itself, as distinct from the engineering it governs.

## 1. Scope of the work

Produce and maintain an architecture description of Gungnir and its system-of-systems
context, governed by principles and contracts, traceable from mission to code, and
current with the repository.

**Included:** the UAF description (plan 03), this tailored ADM set (plan 10), the mission
and capability content the views rest on (plans 02, 04, 05), and the requirements
specification.

**Excluded:** enterprise architecture beyond the product; the engineering work itself,
which the closure roadmap governs; accreditation packages for a specific customer.

## 2. The schedule is the plan set

The architecture work is delivered by ten plans, not by a separate architecture project.
Their sequencing and dependencies are in
[`../../../plans/README.md`](../../../plans/README.md). As of 2026-09-04 all ten have
first drafts.

| Plan | Contribution to the architecture description |
|---|---|
| 02 Mission analysis | The operational content the strategic and operational views describe |
| 04 Mission capabilities | Fifty-six leaf capabilities, the taxonomy every view indexes on |
| 05 Capability design gaps | The gap register, now 83 entries: the work packages of phase E |
| 03 UAF views | The architecture landscape and the element registry |
| 06 UX designs by role | The human-facing part of the business architecture |
| 07 Test-track suite | The data that makes verification claims checkable |
| 01 Product business plan | Phase A business context and the drivers |
| 09 ML model integration | A technology-architecture increment with its own governance |
| 08 AI agent integration | An application-architecture increment with its own boundaries |
| 10 TOGAF ADM documentation | This set: principles, governance, phases, requirements, contracts |

Effort for plan 10 is estimated at 20 to 30 agent-assisted days, of which the owner-held
share is the principles, the governance framework, and the contracts.

## 3. Roles and responsibilities

| Party | Responsibility | Sign-off |
|---|---|---|
| Owner | Sponsor, architecture board, lead architect | Principles, contracts, phase completion, dispensations |
| Architecture agent | Drafting, generation, traceability, compliance runs | None; drafts only |
| Engineering reviewer | Accuracy of phases C and D; compliance finding severity | Phases C and D |
| Domain reviewer | Doctrine, threads, vignettes, roles | Phase B |
| Implementer and reviewer agents | Apply the contracts at the point of change | None |

## 4. Deliverables

The twenty-three documents listed in
[`../README.md`](../README.md), plus the maintenance of the UAF registry and views that
this set references.

## 5. Acceptance criteria

Taken from plan 10 and assessed in [`../README.md`](../README.md):

1. Every tailored deliverable exists and names the views it references.
2. Every principle is traceable to at least one contract in the review checklist.
3. The compliance assessment has run against the code and its findings are in the gap
   register or in the open-items ledger.
4. Every requirement traces to a source and to a view or a code path.
5. A reader unfamiliar with the project can follow the Architecture Definition Document
   to the views and to the code.

Criterion 2 is the one that keeps this set from being decorative: a principle nobody
checks is not governance.

## 6. Assumptions

- The executing team is one lead directing agents, with reviewers available on request.
- The UAF views remain the home of architecture content; these documents reference them.
- The scope lock of D-01 holds, so the transition architectures describe one release
  delivered in four increments rather than a scope negotiation.
- Everything stays unclassified and openly sourced.

## 7. Risks to the architecture work

| Risk | Mitigation |
|---|---|
| Documentation weight exceeds engineering value for a team of one | The tailoring omits or merges nine TOGAF deliverables, each with a stated reason |
| TOGAF and UAF content diverge | No view is redrawn here; phase documents reference them |
| The description drifts from the code | Generated views, a registry check in continuous integration, and a compliance assessment per increment |
| The principles are written and never signed | Owner sign-off is criterion 2 and is not deemed complete by drafting |

## 8. What is explicitly not committed

No architecture tool will be procured. No architecture contract with an external supplier
will be signed as part of this work. No accreditation body has been engaged, and the
compliance evidence package described in the release governance document is designed for
one rather than accepted by one.

## Traceability

`../../../plans/10-togaf-adm-documentation.md`; `../../../plans/README.md`;
`../preliminary/tailored-adm.md`; `../preliminary/governance-framework.md`.

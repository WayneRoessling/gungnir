# TOGAF ADM documentation

Deliverables of [plan 10](../../plans/10-togaf-adm-documentation.md): the tailored
Architecture Development Method, the Preliminary phase and phases A through H,
requirements management, and the Architecture Definition Document.

**The architecture content lives in [`../uaf/`](../uaf/README.md); these documents govern
it and reference the views rather than redrawing them.** Nine of TOGAF's named
deliverables are omitted or merged, each with a stated reason
([`preliminary/tailored-adm.md`](preliminary/tailored-adm.md) §2).

Status: first draft 2026-09-04. The seventeen principles and seventeen contracts were
**signed by the owner on 2026-09-05**, with five findings recorded at signature (see the
findings table in `preliminary/architecture-principles.md`); C-07 and C-11 were run
against the tree that day and both passed. The governance framework is owner-approved
content. Phases B, C, and D await their reviewers. The compliance assessment is the exception: it ran today against the
code and its results are measurements, not proposals.

Start with [`architecture-definition-document.md`](architecture-definition-document.md).

## The set

| Document | Content |
|---|---|
| [`architecture-definition-document.md`](architecture-definition-document.md) | The consolidated description: what Gungnir is, the five shaping decisions, business, data, application, and technology in summary, what is not built, and what to be sceptical about |
| [`framework-cross-reference.md`](framework-cross-reference.md) | Thirty-six DoDAF views mapped to the UAF view that answers each, with the three places the frameworks do not line up |
| **Preliminary** | |
| [`preliminary/architecture-principles.md`](preliminary/architecture-principles.md) | Seventeen principles, business, data, application, technology, each with a rationale, implications, and the contract that enforces it |
| [`preliminary/tailored-adm.md`](preliminary/tailored-adm.md) | What is produced, merged, and omitted; iterations as engineering increments; who does what; what makes an iteration complete |
| [`preliminary/architecture-repository.md`](preliminary/architecture-repository.md) | The eight TOGAF repository classes mapped to folders; the rule against duplication; what is generated and what is written |
| [`preliminary/governance-framework.md`](preliminary/governance-framework.md) | The board of one, decision rights, the review pipeline as the compliance mechanism, escalation, dispensations, and the two rules that cannot be dispensed |
| [`preliminary/capability-assessment.md`](preliminary/capability-assessment.md) | Ten dimensions scored honestly: level 2 overall, traceability at 4, stakeholder engagement at 1 |
| **Phase A** | |
| [`phase-a-vision/architecture-vision.md`](phase-a-vision/architecture-vision.md) | The problem, the stakeholders, the vignettes as business scenarios, the value proposition, the target at one level, the constraints, and what would falsify it |
| [`phase-a-vision/statement-of-architecture-work.md`](phase-a-vision/statement-of-architecture-work.md) | Scope, the plan set as the schedule, roles, deliverables, acceptance criteria, assumptions, risks |
| [`phase-a-vision/stakeholder-map.md`](phase-a-vision/stakeholder-map.md) | Eight roles and eleven external stakeholders, each concern mapped to the view that answers it, and the gap that none has been interviewed |
| **Phase B** | |
| [`phase-b-business/business-architecture.md`](phase-b-business/business-architecture.md) | Roles, capabilities, the ten threads, and the twenty-four mission gaps in five clusters |
| **Phase C** | |
| [`phase-c-information-systems/data-architecture.md`](phase-c-information-systems/data-architecture.md) | The canonical model, provenance as a field, the journal, lifecycle and retention, data security, standards, six data gaps |
| [`phase-c-information-systems/application-architecture.md`](phase-c-information-systems/application-architecture.md) | Fifty crates in seven layers, the service boundary, what is real and what is scaffold, fifty-nine technical gaps by layer |
| **Phase D** | |
| [`phase-d-technology/technology-architecture.md`](phase-d-technology/technology-architecture.md) | Platforms, the three profiles, the pinned version set, the two graphics contexts, standards, assurance, ten technology gaps |
| **Phase E** | |
| [`phase-e-opportunities-solutions/work-packages.md`](phase-e-opportunities-solutions/work-packages.md) | Eighteen work packages over three increments, the four sequencing constraints, and which two packages are cuttable |
| [`phase-e-opportunities-solutions/transition-architectures.md`](phase-e-opportunities-solutions/transition-architectures.md) | T1 observed, T2 to T4 targeted: what becomes real, what stays scaffold, what the health flags should say |
| **Phase F** | |
| [`phase-f-migration/implementation-and-migration-plan.md`](phase-f-migration/implementation-and-migration-plan.md) | Twelve steps with their gates, resources, dependencies outside the code, six delivery risks, and why there are no dates |
| **Phase G** | |
| [`phase-g-implementation-governance/architecture-contracts.md`](phase-g-implementation-governance/architecture-contracts.md) | Seventeen contracts with their checks and automatability; where each is applied; the two that cannot be dispensed |
| [`phase-g-implementation-governance/compliance-assessment.md`](phase-g-implementation-governance/compliance-assessment.md) | **Run 2026-09-04.** Six passes, one observation, two findings, one not verifiable, and what was not checked |
| **Phase H** | |
| [`phase-h-change-management/change-management.md`](phase-h-change-management/change-management.md) | What counts as an architecture change, the three drivers, the six-step process, the twenty-nine decisions on record, value realization, six open requests |
| **Requirements management** | |
| [`requirements-management/architecture-requirements-specification.md`](requirements-management/architecture-requirements-specification.md) | Fifty-eight requirements in seven categories, each traced to a source, a view, a crate or gap, and a verification gate |
| [`requirements-management/requirements-repository.md`](requirements-management/requirements-repository.md) | Where a requirement lives, the identifier scheme, the traceability chain, what is not yet traced, change history, and two tensions |

## What the compliance assessment found

It ran against the workspace, not against an intention:

| Check | Result |
|---|---|
| Dependency direction, 147 crate-to-crate edges | Pass |
| Single owning crate, nine shared types | Pass |
| Recorded stack, twenty dependencies | Pass |
| No `unwrap()` or `expect()` outside tests, 148 source files | Pass, zero |
| Document and section citations from code, 143 of them | Pass, all resolve |
| Relative links across 291 Markdown files | Pass, zero broken |
| Are any of these checks automated? | **No.** Finding CA-F1 |
| Is any `todo!()` reachable at runtime? | **Unproven.** Finding CA-F2 |

Both findings are filed: GAP-081 automates the checks, GAP-082 settles the reachability
question. A third, smaller finding, CA-F3, is a sentence missing from the standards
document about where the 3D-data crates sit in the layer order.

## Decisions recorded here

| Question plan 10 left open | Answer |
|---|---|
| Must an external customer framework be mapped alongside TOGAF and UAF? | A DoDAF cross-reference table, and nothing further committed. A customer requiring a different national framework raises a change request; the registry makes the additional views tractable |
| What is the cadence of compliance assessments once the repository is hosted? | Every increment boundary and whenever a principle or contract changes. Once GAP-081 lands the mechanical part runs on every change, and the assessment records only what a person had to judge |

## What this set is careful about

- **A principle with no contract is an aspiration.** All seventeen have one, and fourteen
  are machine-checkable.
- **Nothing here redraws a view.** Where a phase document summarizes, the citation is on
  the same line so the summary can be checked against its home.
- **The transition architectures carry no dates.** The business plan owns the schedule; a
  second schedule here would disagree with it within a month.

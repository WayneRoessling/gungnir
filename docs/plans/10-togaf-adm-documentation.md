# Plan 10: TOGAF ADM documentation

## Purpose

Produce the full set of TOGAF Architecture Development Method (ADM) deliverables for
Gungnir, tailored to a small agent-assisted team, so that the architecture is
governed (principles, requirements, contracts, compliance) and its evolution is
managed (transition architectures, migration plan, change management). The UAF views
from plan 03 supply the architecture content; the TOGAF documents supply the
governance around it and reference the views rather than redrawing them.

## Scope

In scope: Preliminary phase, phases A through H, Requirements Management, the
Architecture Definition Document, the Architecture Requirements Specification, the
transition architectures, the architecture repository structure, and the compliance
assessment against the current code. Tailoring is explicit: which artefacts are
produced, which are merged, which are omitted and why.

Out of scope: enterprise-wide architecture beyond the Gungnir product and its
system-of-systems interfaces.

## Inputs

- Plans 02 through 06 and 08 and 09 for content; plan 03 for the views.
- `ARCHITECTURE.md`, `docs/gungnir-capabilities.md`, `docs/agentic-coding-standards.md`,
  `docs/release-governance.md`, `docs/agentic-workflow.md`, `CONTRIBUTING.md`,
  `docs/verification-capability-table.md`, `docs/performance-budgets.md`.
- The TOGAF Standard (The Open Group) for phase definitions and deliverable names.

## Deliverables and target location

All under `docs/architecture/togaf/`:

| Path | Content |
|---|---|
| `README.md` | Tailored ADM: which deliverables exist, how iterations map to engineering increments, how UAF views are referenced |
| `preliminary/architecture-principles.md` | Business, data, application, technology principles; each with statement, rationale, implications; several already exist implicitly (one-way dependencies, recommendation-only, honest health, single model) |
| `preliminary/tailored-adm.md` | Which phases and artefacts, iteration cadence, roles |
| `preliminary/architecture-repository.md` | Structure of the repository: this folder, the UAF model, the standards information base (`gungnir-interop` catalogue), the reference library (the existing docs), the governance log |
| `preliminary/governance-framework.md` | Architecture board (the owner and reviewers), decision rights, the agent review pipeline as the compliance mechanism, escalation |
| `preliminary/capability-assessment.md` | Architecture maturity of the team and process today |
| `phase-a-vision/architecture-vision.md` | Problem, stakeholders and concerns (from plan 02 roles and plan 01 buyers), business scenarios (the vignettes), value proposition, high-level target architecture (UAF Sm-Ov), constraints |
| `phase-a-vision/statement-of-architecture-work.md` | Scope, schedule (the plan set), roles, acceptance |
| `phase-a-vision/stakeholder-map.md` | Stakeholders, concerns, the views that address each concern |
| `phase-b-business/business-architecture.md` | Organization and roles (UAF Pr views), business capabilities (plan 04), processes (UAF Op-Pr), business gap analysis (plan 05 mission gaps) |
| `phase-c-information-systems/data-architecture.md` | The canonical model (UAF If views), the interface control document, the journal, data lifecycle and retention, data security |
| `phase-c-information-systems/application-architecture.md` | Crates, binaries, services (UAF Sv and Rs views), the service boundary, application gap analysis (plan 05 technical gaps by layer) |
| `phase-d-technology/technology-architecture.md` | Platforms, the deployment profiles (UAF Ar views), the two GPU contexts, the stack and version set, technology gap analysis |
| `phase-e-opportunities-solutions/work-packages.md` | Work packages from the gap register and the plan set; transition architectures per increment |
| `phase-e-opportunities-solutions/transition-architectures.md` | The architecture at the end of each increment, what is real and what is scaffold, using `ARCHITECTURE.md` §10 as the ledger |
| `phase-f-migration/implementation-and-migration-plan.md` | Roadmap, dependencies, resources, the suggested implementation order from `README.md`, risks |
| `phase-g-implementation-governance/architecture-contracts.md` | The contracts every change must honour: dependency direction, one owning crate per type, honest health, stack sign-off, pass criteria; expressed as the review checklist |
| `phase-g-implementation-governance/compliance-assessment.md` | Assessment of the current code and documents against the principles and contracts, with findings |
| `phase-h-change-management/change-management.md` | How architecture changes are requested and decided, the scope-lock decision, value realization measures |
| `requirements-management/architecture-requirements-specification.md` | Requirements with identifiers, sources (mission threads, capabilities, standards, security), and traceability to views and code |
| `requirements-management/requirements-repository.md` | How requirements are stored and traced (the UAF registry plus this file), change history |
| `architecture-definition-document.md` | The consolidated ADD: summary of phases B, C, and D with references to the UAF views |

## Tailoring decisions (initial)

- Iterations follow the engineering increments; each increment closes with a
  transition architecture and a compliance assessment.
- Business, information-systems, and technology architecture content is held in
  the UAF views; the TOGAF phase documents are short, reference the views, and add
  the gap analyses and the governance content.
- The agent review pipeline in `docs/agentic-workflow.md` is the compliance
  mechanism; architecture contracts are the checklist it runs.
- The architecture repository is this folder plus the UAF model; no separate tool.
- Risk management is shared with `docs/mission/gap-analysis/` and the business risk
  register; the ADM documents reference them.

## Method

1. **Preliminary.** Write the principles (harvesting the ones the code already
   enforces), the tailored ADM, the repository structure, and the governance
   framework; owner approval.
2. **Phase A.** Vision and statement of work from plans 01 and 02; stakeholder map
   from the roles.
3. **Phases B, C, D.** Short documents referencing the UAF views; gap analyses
   imported from plan 05.
4. **Phases E and F.** Work packages and transition architectures from the gap
   register and the increments; the migration plan from the implementation order.
5. **Phase G.** Architecture contracts as the checklist; run the compliance
   assessment against the current code with an agent and record findings.
6. **Phase H and requirements.** Change management process and the requirements
   specification with traceability.
7. **ADD.** Consolidate; owner review; publish.
8. **Cadence.** Repeat the compliance assessment and transition architecture at
   each increment.

## Roles

- Owner (architecture board): principles, governance, contracts, approvals.
- Architecture agent: drafting, traceability, compliance assessment runs.
- Engineering reviewer: phases C and D accuracy, compliance findings.

## Dependencies

Plan 03 (views) and plans 02, 04, 05 (content); plan 01 for phase A's business
context. Last in the sequence.

## Effort and sequencing

20 to 30 agent-assisted days; 4 weeks elapsed after plan 03.

## Acceptance criteria

- Every tailored deliverable exists and states which UAF views it references.
- The principles are approved and each is traceable to at least one contract in
  the review checklist.
- The compliance assessment has run against the code and its findings are in the
  gap register or `ARCHITECTURE.md` §10.
- The requirements specification traces every requirement to a source and to a
  view or code path.
- A reader unfamiliar with the project can follow the ADD to the views and the
  code.

## Risks

- Documentation weight for a small team; mitigate with the tailoring decisions and
  by generating from the registry and code wherever possible.
- Divergence between TOGAF and UAF content; mitigate by never duplicating a view.

## Open questions

- Whether an external accreditation or customer framework (for example a national
  defense architecture framework) must be mapped in addition to TOGAF and UAF.
- The cadence of compliance assessments once the repository is hosted.

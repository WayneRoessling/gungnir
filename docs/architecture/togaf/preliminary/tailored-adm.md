# The tailored Architecture Development Method

Status: first draft, 2026-09-04. Which parts of the ADM this project runs, which it
merges, which it omits, and what an iteration is.

## 1. Why tailoring is the first decision

TOGAF's full deliverable set assumes an enterprise architecture function. This project is
one owner directing agents, with reviewers on call. Applied unmodified the method would
produce more documents than the code, and the documents would rot first. The tailoring
below keeps the parts that change engineering behaviour and drops the parts that would
only restate the views.

The test applied to every candidate deliverable: **does it change what someone does, or
does it only describe what another document already holds?** Only the first kind is
produced.

## 2. What is produced, merged, and omitted

| TOGAF deliverable | Here | Why |
|---|---|---|
| Architecture Principles | Produced, `preliminary/architecture-principles.md` | Seventeen principles, each with an enforcing contract |
| Architecture Repository | Produced, `preliminary/architecture-repository.md` | The folders are the repository; the structure needs stating once |
| Architecture Governance Framework | Produced, `preliminary/governance-framework.md` | Decision rights for a one-person board are not obvious |
| Architecture Capability Assessment | Produced, `preliminary/capability-assessment.md` | Honest maturity, so the roadmap is not written for a team that does not exist |
| Request for Architecture Work | **Omitted** | The owner is the sponsor and the architect; the plan set is the request |
| Architecture Vision | Produced, `phase-a-vision/architecture-vision.md` | |
| Statement of Architecture Work | Produced, `phase-a-vision/statement-of-architecture-work.md` | Scope and acceptance for the plan set |
| Stakeholder Map | Produced, `phase-a-vision/stakeholder-map.md` | Concern-to-view mapping is the one thing the views do not carry |
| Communications Plan | **Omitted** | One team, one repository |
| Business Architecture | Produced as a short document over the operational and personnel views | Content stays in the views |
| Data Architecture | Produced as a short document over the information views | As above |
| Application Architecture | Produced as a short document over the services and resources views | As above |
| Technology Architecture | Produced as a short document over the actual-resources views | As above |
| Architecture Definition Document | Produced, `architecture-definition-document.md` | The consolidated entry point a newcomer reads |
| Architecture Requirements Specification | Produced, `requirements-management/` | With identifiers and traceability |
| Architecture Roadmap and Work Packages | Produced, `phase-e-opportunities-solutions/work-packages.md` | Derived from the gap register, not written fresh |
| Transition Architectures | Produced, `phase-e-opportunities-solutions/transition-architectures.md` | One per increment |
| Implementation and Migration Plan | Produced, `phase-f-migration/` | |
| Implementation Governance Model | Merged into `phase-g-implementation-governance/architecture-contracts.md` | The review pipeline already is the model |
| Architecture Contract | Produced as the contract set C-01 to C-17 | Expressed as review checks rather than as a signed document with a supplier |
| Compliance Assessment | Produced, `phase-g-implementation-governance/compliance-assessment.md` | Run against the code, not asserted |
| Change Request and Architecture Change Management | Produced, `phase-h-change-management/change-management.md` | |
| Business Transformation Readiness Assessment | **Omitted** | No organization is being transformed; the product is greenfield |
| Capability Maturity models beyond the assessment | **Omitted** | Would not change any decision this year |
| Architecture Board terms of reference as a separate document | Merged into the governance framework | |

Nine of TOGAF's named deliverables are omitted or merged. Each omission is a decision
with a reason, not an oversight, so that a reviewer can challenge the specific one they
care about.

## 3. Iterations map to engineering increments

The ADM's iteration cycles are not run on a calendar here. One iteration is one
engineering increment from `gungnir-capabilities.md` §7.

| Increment | ADM emphasis | Closes with |
|---|---|---|
| I1 Productize the core (done) | Phases B, C, D: the canonical model and the service boundary | The architecture as described in `ARCHITECTURE.md` §7 |
| I2 Integrate real data | Phases C and D: ingest, time, interoperability, harnesses | Transition architecture T2 and a compliance assessment |
| I3 Close the decision loop | Phases B and C: policy, command, assessment, the role workspaces | Transition architecture T3 and a compliance assessment |
| I4 Operationalize and scale | Phases D and F: security, the interface, observability, the node | Transition architecture T4, a compliance assessment, and the release evidence package |

Phases E, F, G, and H run continuously rather than once: the work packages are the gap
register, the migration plan is the closure roadmap, governance is the review pipeline on
every change, and change management is the decision log.

Requirements management runs across all of them, as the ADM intends.

## 4. Who does what

| Role | Held by | Responsibility |
|---|---|---|
| Architecture board | The owner | Principles, contracts, dispensations, phase approvals |
| Lead architect | The owner | Phase A, the vision, tailoring decisions |
| Architecture agent | An agent under direction | Drafting, traceability, generated views, compliance runs |
| Engineering reviewer | On call | Phases C and D accuracy, compliance findings, severity calls |
| Domain reviewer | On call | Phase B content: doctrine, threads, vignettes |
| Implementer and reviewer agents | Per change | The contracts, at the point of change |

The board of one is a stated risk, recorded in the governance framework, not a pretence
of separation.

## 5. What makes an iteration complete

An increment closes when, and only when:

1. Every work package targeted at it is closed or explicitly deferred with a reason.
2. Its transition architecture states what became real, what is still scaffold, and what
   the honest health flags say.
3. A compliance assessment has run and its findings are in the gap register or in
   `ARCHITECTURE.md` §10.
4. Every measure whose target the increment claims has a harness and a recorded result.
5. `ARCHITECTURE.md` §10 has moved the items the increment resolved.

Point 4 is the one most likely to be skipped under pressure, and it is the reason AP-17
exists.

## 6. What this tailoring does not do

It does not produce a separate architecture repository tool, a formal architecture
contract with an external supplier, or an accreditation package. If a customer requires a
national defence architecture framework view, the mapping in
[`../framework-cross-reference.md`](../framework-cross-reference.md) is the starting
point and the additional views are a change request, not a rewrite.

## Traceability

`../../../plans/10-togaf-adm-documentation.md`; `../../../gungnir-capabilities.md` §7;
`../../../agentic-workflow.md`; `../../uaf/README.md` for the views this set references.

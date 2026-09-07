# Stakeholder map

Status: first draft, 2026-09-04. Phase A. Who has a stake, what they need to be
convinced of, and which view answers it.

This is the one artefact the UAF set does not carry: the views describe the architecture,
but nothing in them says which view a given reader should open. TOGAF calls that
concern-to-viewpoint mapping, and it is the reason this document exists.

## 1. Internal stakeholders, the eight roles

| Stakeholder | Concern | View or document that addresses it | Satisfied today? |
|---|---|---|---|
| Operator | Can I see the threat, believe the picture, and decide inside the time available? | `../../uaf/operational/Op-Is-VG-01.md`, `Op-Pr-MT-01.md`; `../../../ux/task-analysis/`; measures MOP-01 to MOP-09 | Partly built (2026-09-05): the status strip, track table and evidence card are on screen, so the picture and its uncertainty are visible; the approval queue and decision dialog are gap GAP-038, and the evidence card has no evidence in it until GAP-010, GAP-019 and GAP-028 |
| Supervisor | Can I manage a queue across domains and hold what needs deconfliction? | `Op-Pr-MT-02.md`; `../../../ux/information-architecture.md`; `../../uaf/personnel/Pr-Cn.md` | Designed. Queue ordering is GAP-035 |
| Commander | Do I keep the authority I am accountable for, and can I delegate it precisely? | `../../../mission/roles-and-stakeholders.md` §4; `../../uaf/security/Sc-Pr.md`; D-15 | Specified. Enforcement is GAP-058 |
| Analyst | Can I reconstruct a session exactly and defend a conclusion from it? | `../../uaf/resources/Rs-St.md`; `Op-Pr-MT-09.md`; CAP-5.2, CAP-5.3 | Journal and replay exist; measures from the journal are GAP-047 |
| Intelligence analyst | Can I fuse evidence into a declaration and release a product with its marking? | `Op-Pr-MT-08.md`; `../../uaf/information/If-Tx.md`; D-06 | Modelled. Releasability enforcement is GAP-062 |
| Sensor manager | Is coverage what I think it is, and what exactly did I lose? | `Op-Pr-MT-07.md`; `../../uaf/resources/Rs-Cn.md`; CAP-1.3, CAP-1.4 | Registry implemented, not wired: GAP-003, GAP-006 |
| Planner | Will this laydown cover the assets that matter, and can I rehearse it? | `Op-Pr-MT-09.md`; `Op-Is-VG-09.md`; CAP-3.1 | Defended-asset list is GAP-026 |
| Administrator | Who holds what authority, and is the record complete and tamper-evident? | `../../uaf/security/Sc-Tx.md`, `Sc-Sr.md`; `../../../release-governance.md` | Traits exist; authentication and audit wiring are GAP-057 and GAP-059 |

## 2. External stakeholders

| Stakeholder | Concern | View or document | Satisfied today? |
|---|---|---|---|
| Higher command | Sector status, warnings, reports, order of battle | `../../../gungnir-api-v1.md`; `Op-Cn.md` | Contract written; transport is GAP-041 |
| Neighbouring sectors | Track and warning exchange, raid handover | `../../uaf/services/Sv-Cn.md`; SD-04 | Codecs are GAP-064; exchange is GAP-065 |
| Fire units and effectors | Receive decided assignments, report readiness and outcomes | `Op-Pr-MT-01.md` step 7; CAP-4.4 | Handoff endpoint is GAP-040; D-08 made endpoints generic |
| Sensor owners and maintainers | Modes, tasking, faults, calibration | `Rs-Cn.md`; CAP-1.3 | Outbound control is GAP-004 |
| Civil aviation and port authorities | Corridors, cooperative data, warnings before engagements | SD-09, SD-10; `../../uaf/information/If-Cn.md` | Decoders are GAP-010 |
| Coalition partners | Picture and product exchange with releasability | SD-04; D-06 | Marking in increment 3, enforcement in increment 4 |
| Accreditors and auditors | Evidence rather than assertion | `../../../release-governance.md`; `../../../verification-capability-table.md`; `../phase-g-implementation-governance/compliance-assessment.md` | Evidence package designed; never presented to an accreditor |
| Integrators and primes | Will it fit what we already own? | `../../../gungnir-api-v1.md`; `Sd-Tx.md`; CAP-7.x | Conformance suite is GAP-063 |
| Buyer or programme office | Cost per engagement, time to decision, sustainment | `../../../business/business-plan.md`; measures MOE-01 to MOE-05 | Claimed, never measured with a customer |
| Investor | Is it buildable by this team, and defensible? | `../../../business/`; `../preliminary/capability-assessment.md` | Drafted; no figure checked by a second reader |
| Owner | Product decisions, technical debt, the truth about status | `ARCHITECTURE.md` §10; the gap register | Current |

## 3. Concerns grouped into viewpoints

Six recurring concerns, and where each is answered across the whole set:

| Concern | Answered by |
|---|---|
| **Can I trust the picture?** | Provenance on every view; source health; staleness; the honest-status rule (AP-02, AP-07) |
| **Who decided, and can you prove it?** | The audit log, the journal, the authority matrix (AP-01, AP-03) |
| **What happens when it degrades?** | `ARCHITECTURE.md` §8.4; VG-07 and VG-10; store-and-forward and reconciliation (AP-05, AP-08) |
| **Will it talk to my systems?** | The standards taxonomy, the schema catalogue, the interface contract (CAP-7.x) |
| **Is it fast enough?** | `performance-budgets.md`; measures MOP-01 to MOP-09; the harnesses are GAP-056 |
| **Can you show me evidence?** | The verification table, the release evidence package, the compliance assessment (AP-16, AP-17) |

## 4. The gap in this map

Every entry above was written from doctrine, open sources, and the code. **No stakeholder
in either table has been interviewed.** The concerns are inferred, and a wrong inference
here propagates into the capability taxonomy and from there into the gap register, which
is the largest single source of error in the architecture description.

The first three conversations worth having, in order: an operator on whether the queue
helps or competes for attention; an integrator on which two interoperability formats
actually matter; an accreditor on whether the evidence package resembles what they ask
for.

## Traceability

`../../../mission/roles-and-stakeholders.md`; `../../uaf/personnel/Pr-Tx.md`;
`../../../ux/personas.md`; `../../../business/market-analysis.md` for the buyer personas;
`architecture-vision.md` §2.

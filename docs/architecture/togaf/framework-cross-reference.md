# Framework cross-reference

Status: first draft, 2026-09-04. One table joining the three frameworks this project
touches, so that a reader who asks in one vocabulary is answered in another without a
second set of diagrams being drawn.

Plan 10 left open whether an external customer framework must be mapped alongside TOGAF
and UAF. The answer taken on 2026-09-04: **produce a DoDAF cross-reference table and
commit to nothing further.** A programme office that asks for DoDAF views gets this
mapping and the views it points at; a customer that requires a different national
framework raises a change request, and the additional views are drawn then, from the same
registry.

## 1. Why a table and not a second view set

Every view in this project is built from one element registry. A DoDAF view of the same
architecture would contain the same elements with different names on the frame. Drawing
them twice creates two things that can disagree, and the second one always rots.

So: one architecture description, three ways in.

- **UAF 1.2** is where the architecture is described.
- **TOGAF 10 ADM** is how it is governed and evolved. Its phase documents reference the
  views.
- **DoDAF 2.02** is the vocabulary a defence customer is most likely to use. This table
  maps it.

## 2. The mapping

| DoDAF view | What it asks for | UAF view here | TOGAF phase |
|---|---|---|---|
| AV-1 Overview and Summary | Scope, purpose, users | [`uaf/summary-and-overview.md`](../uaf/summary-and-overview.md) plus [`phase-a-vision/architecture-vision.md`](phase-a-vision/architecture-vision.md) | A |
| AV-2 Integrated Dictionary | The definitions everything else uses | [`uaf/model/elements.yaml`](../uaf/model/elements.yaml) and [`mission/glossary.md`](../../mission/glossary.md) | Preliminary |
| CV-1 Vision | Strategic intent | `phase-a-vision/architecture-vision.md` | A |
| CV-2 Capability Taxonomy | The capability hierarchy | [`uaf/strategic/St-Tx.md`](../uaf/strategic/St-Tx.md) | B |
| CV-3 Capability Phasing | Capability by increment | [`uaf/strategic/St-Rm.md`](../uaf/strategic/St-Rm.md) | E |
| CV-4 Capability Dependencies | Which capability needs which | [`uaf/strategic/St-Sr.md`](../uaf/strategic/St-Sr.md) | B |
| CV-5 Capability to Organization | Who provides what | [`uaf/strategic/St-Cn.md`](../uaf/strategic/St-Cn.md) and [`uaf/personnel/Pr-Cn.md`](../uaf/personnel/Pr-Cn.md) | B |
| CV-6 Capability to Operational Activity | Capability to activity | [`uaf/traceability/capability-to-activity.md`](../uaf/traceability/capability-to-activity.md) | B |
| CV-7 Capability to Services | Capability to service | [`uaf/traceability/activity-to-service.md`](../uaf/traceability/activity-to-service.md) | C |
| OV-1 High-Level Operational Concept | The picture a general reads | [`uaf/operational/Op-Cn.md`](../uaf/operational/Op-Cn.md) with the vignettes | A |
| OV-2 Operational Resource Flow | Who exchanges what with whom | [`uaf/operational/Op-Cn.md`](../uaf/operational/Op-Cn.md) | B |
| OV-3 Operational Resource Flow Matrix | The same, as a matrix | [`uaf/operational/Op-If.md`](../uaf/operational/Op-If.md) | B |
| OV-4 Organizational Relationships | Roles and their relationships | [`uaf/personnel/Pr-Sr.md`](../uaf/personnel/Pr-Sr.md), [`Pr-Tx.md`](../uaf/personnel/Pr-Tx.md) | B |
| OV-5a Operational Activity Decomposition | The activity tree | [`uaf/operational/Op-Tx.md`](../uaf/operational/Op-Tx.md) | B |
| OV-5b Operational Activity Model | Activity flows | [`uaf/operational/Op-Pr-MT-01.md`](../uaf/operational/Op-Pr-MT-01.md) to `Op-Pr-MT-10` | B |
| OV-6a Operational Rules | Rules of engagement and constraints | The rules-of-engagement structure in the mission set; [`uaf/security/Sc-Pr.md`](../uaf/security/Sc-Pr.md) | B |
| OV-6b State Transition | The picture's states | [`uaf/operational/Op-St.md`](../uaf/operational/Op-St.md) | B |
| OV-6c Event-Trace | Scenario sequences | [`uaf/operational/Op-Is-VG-01.md`](../uaf/operational/Op-Is-VG-01.md) to `Op-Is-VG-10` | B |
| SV-1 Systems Interface | Systems and their interfaces | [`uaf/resources/Rs-Tx.md`](../uaf/resources/Rs-Tx.md), [`Rs-Sr.md`](../uaf/resources/Rs-Sr.md) | C |
| SV-2 Systems Resource Flow | What connects to what | [`uaf/resources/Rs-Cn.md`](../uaf/resources/Rs-Cn.md) | C |
| SV-4 Systems Functionality | What each system does | [`uaf/resources/Rs-Pr.md`](../uaf/resources/Rs-Pr.md) | C |
| SV-6 Systems Resource Flow Matrix | The exchanges, as a matrix | [`uaf/resources/Rs-If.md`](../uaf/resources/Rs-If.md) | C |
| SV-7 Systems Measures | Performance parameters | [`uaf/parameters/Pm-Me.md`](../uaf/parameters/Pm-Me.md) and [`performance-budgets.md`](../../performance-budgets.md) | D |
| SV-10b Systems State Transition | System states | [`uaf/resources/Rs-St.md`](../uaf/resources/Rs-St.md) | C |
| SvcV-1 Services Context | The service set | [`uaf/services/Sv-Tx.md`](../uaf/services/Sv-Tx.md), [`Sv-Sr.md`](../uaf/services/Sv-Sr.md) | C |
| SvcV-2 Services Resource Flow | Service connectivity | [`uaf/services/Sv-Cn.md`](../uaf/services/Sv-Cn.md) | C |
| SvcV-4 Services Functionality | Service behaviour | [`uaf/services/Sv-Pr.md`](../uaf/services/Sv-Pr.md) | C |
| SvcV-6 Services Resource Flow Matrix | Service exchanges | [`uaf/services/Sv-If.md`](../uaf/services/Sv-If.md) | C |
| DIV-1 Conceptual Data Model | The information concepts | [`uaf/information/If-Tx.md`](../uaf/information/If-Tx.md) | C |
| DIV-2 Logical Data Model | The logical structure | [`uaf/information/If-Sr.md`](../uaf/information/If-Sr.md) | C |
| DIV-3 Physical Data Model | The realized schema | [`uaf/information/If-Cn.md`](../uaf/information/If-Cn.md) and the canonical schema | C |
| StdV-1 Standards Profile | Standards in force | [`uaf/standards/Sd-Tx.md`](../uaf/standards/Sd-Tx.md) | D |
| StdV-2 Standards Forecast | Standards planned | [`uaf/standards/Sd-Rm.md`](../uaf/standards/Sd-Rm.md) | D |
| PV-1 Project Portfolio | The projects | [`plans/README.md`](../../plans/README.md) | E |
| PV-2 Project Timelines | Sequencing | [`uaf/projects/Pj-Rm.md`](../uaf/projects/Pj-Rm.md) | F |
| PV-3 Project to Capability | Which project delivers what | [`uaf/projects/Pj-Rm.md`](../uaf/projects/Pj-Rm.md) with the capability roadmap | E |

## 3. Where the frameworks do not line up

Three honest mismatches, worth stating rather than papering over:

**DoDAF has no security viewpoint.** UAF does, and this project uses it: five security
views cover the taxonomy, structure, connectivity, and processes of authority, audit, and
threat. A DoDAF-only reader would find these under OV-6a operational rules and SV-1, which
loses most of the content. If a customer requires DoDAF strictly, the security views are
delivered as supplementary material rather than folded into a viewpoint that does not fit.

**UAF actual resources have no clean DoDAF home.** The deployed-configuration views map
loosely onto SV-1 and SV-2, but DoDAF does not distinguish the designed resource from the
fielded one. The deployment profiles are the content, and they matter here more than the
distinction does.

**TOGAF phases and DoDAF views answer different questions.** The phase column above says
which phase document discusses a view, not that the view belongs to the phase. A view has
one home, in the UAF set.

## 4. What is not claimed

This is a cross-reference, not a DoDAF-conformant architecture description. No DoDAF
view has been produced in DoDAF's own presentation conventions, no DoDAF meta-model
conformance has been asserted, and no accreditor has seen it. If conformance is required
it is a change request with its own effort estimate, and the registry is what makes it
tractable.

## Traceability

`../uaf/README.md` for the view grid; `preliminary/tailored-adm.md` for the TOGAF
tailoring; `phase-h-change-management/change-management.md` for how a customer framework
requirement would enter.

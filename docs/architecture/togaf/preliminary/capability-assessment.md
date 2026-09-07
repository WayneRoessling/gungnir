# Architecture capability assessment

Status: first draft, 2026-09-04. How mature the architecture practice actually is today,
scored honestly, so that the roadmap is not written for a team that does not exist.

Scale, TOGAF's usual five levels: 0 none, 1 initial (ad hoc), 2 repeatable (documented,
inconsistently applied), 3 defined (documented and applied), 4 managed (measured), 5
optimizing (measured and improved on evidence).

## 1. Scores

| Dimension | Score | Evidence for the score | What would raise it |
|---|---|---|---|
| Architecture description | 3 | Fifty-eight UAF views on one registry, a consistency check in continuous integration, a drawn dependency graph derived from the manifests | Views reviewed by someone other than their author |
| Principles and contracts | 2 | Seventeen principles written today, fourteen machine-checkable, none yet signed | Owner sign-off, then the checks running automatically |
| Governance | 2 | Decision rights defined, a review pipeline that runs per change, an adversarial reviewer checklist | A second architect; the pipeline running on hosted runners |
| Requirements management | 2 | Requirements exist across capabilities, measures, and standards, and are now given identifiers here | Requirement identifiers traced from code, and change history kept |
| Traceability | 4 | Capability to activity to service to resource to standard, generated and checked; capability to crate; gap to capability; measure to capability | Requirements and test results joined to the same chain |
| Change management | 3 | An open-items ledger, a decision log with seventeen engineering and twelve business decisions, all outcomes recorded | Change requests raised and closed through a tracker rather than a document |
| Compliance verification | 2 | The first assessment ran today against the code and passed six of eight checks | The checks running in continuous integration rather than by hand |
| Skills and capacity | 1 | One person, agent-assisted; reviewers on call and not yet engaged | Named reviewers with time committed |
| Tooling | 3 | Markdown, PlantUML, Mermaid, and Python generators in the repository; no architecture tool and none wanted | Rendering exercised, which it has not been |
| Stakeholder engagement | 1 | Stakeholders identified and their concerns mapped; none consulted | Any conversation with a real operator or buyer |

Weighted honestly, the practice sits at **level 2, repeatable**, with two dimensions
notably ahead (traceability, description) and two notably behind (skills, stakeholder
engagement).

## 2. What the scores mean in practice

**The description is stronger than the practice.** Fifty-eight views and a checked
registry would normally imply an architecture function. Here they imply an agent that
writes quickly and a check that catches inconsistency. That is worth something real, and
it is not the same thing as an architecture that people have argued about.

**Traceability at 4 is genuine and is the asset.** The chain from a mission thread to a
capability to a service to a crate to a verification row exists, is generated, and fails
the build when it breaks. It is the reason a newcomer can start anywhere and reach the
code, and it is what plan 10 is building on rather than replacing.

**Stakeholder engagement at 1 is the honest weak point.** Eight roles and eleven external
stakeholder groups are described from doctrine and open sources. No operator has used the
product, no buyer has quoted a price, and no accreditor has seen the evidence package.
Three of the plan sets say so in their own status lines, and this assessment agrees with
them rather than averaging them away.

**Skills at 1 is a capacity statement, not a competence one.** The work exists; the
second reader does not.

## 3. Risks that follow from the scores

| Risk | From | Where it is tracked |
|---|---|---|
| The architecture is self-consistent and wrong about the mission | Stakeholder engagement 1 | `mission/mission-analysis.md` §11 assumptions; business risk register |
| A principle erodes because nothing enforces it automatically | Compliance verification 2 | Finding CA-F1, new gap |
| A single person is the board, the architect, and the reviewer | Skills 1 | Governance framework §1; business risk register |
| Diagrams are never rendered and a syntax error goes unnoticed | Tooling 3 | `../../uaf/README.md`; rendering not exercised on the drafting host |

## 4. What is deliberately not pursued

Levels 4 and 5 across the board would mean measuring the architecture practice itself.
For a team of one that is ceremony. The dimensions worth taking to 4 are the two that
change engineering outcomes: **compliance verification**, by automating the checks, and
**requirements management**, by tracing requirement identifiers from code. Both are gaps
filed by this plan.

## Traceability

`governance-framework.md`; `../phase-g-implementation-governance/compliance-assessment.md`;
`../../uaf/README.md`; `../../../business/risk-register.md`;
`../../../mission/mission-analysis.md` §11.

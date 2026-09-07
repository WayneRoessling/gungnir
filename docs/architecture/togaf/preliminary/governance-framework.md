# Architecture governance framework

Status: first draft, 2026-09-04. **Owner-approved content.** Who decides architecture
questions, how compliance is checked, and what happens when a change wants to break a
principle.

## 1. The board

The architecture board is the owner, with two reviewers on call: an engineering reviewer
for the technology and application phases, and a domain reviewer for the business phase.

This is a board of one, and that is a governance weakness rather than a simplification.
It is recorded here so it is visible in the maturity assessment and in the risk register,
and it is partly compensated by two things that do not depend on headcount:

1. **The contracts are machine-checkable.** Fourteen of the seventeen contracts are
   checks a script or a compiler can run, so compliance does not rest on one person
   remembering.
2. **The reviewer agent is adversarial by instruction.** It runs a fixed checklist and is
   prompted to look for violations rather than to confirm the change.

Neither replaces a second architect. The first hire that changes this is named in the
business plan's team section.

## 2. Decision rights

| Decision | Decider | Consulted | Recorded in |
|---|---|---|---|
| Add or change an architecture principle | Owner | Engineering reviewer | This folder, then the contract set |
| Add a dependency edge | Owner | Engineering reviewer | `ARCHITECTURE.md` graph and §7.1, same change |
| Add a crate to the stack | Owner | Engineering reviewer | `agentic-coding-standards.md` §2.9 |
| Change a pass criterion or a measure target | Owner | The measure's owner | `verification-capability-table.md` or the measures catalogue, as a change request |
| Move a capability between increments | Owner | Plan lead | The gap register and the closure roadmap |
| Promote a verification row to a gate | Owner | Engineering reviewer | `architecture.md` crate map and the table |
| Accept a compliance finding as a gap | Owner | Engineering reviewer | The gap register |
| Grant a dispensation | Owner | Both reviewers | Section 5 below, with an expiry |
| Anything in the human-owned tier | Owner, personally | | The change description, with the reason |
| Release a version | Owner | | The release evidence package |

Agents decide nothing on this list. They draft, they check, and they raise.

## 3. Compliance is the review pipeline

TOGAF's compliance function is not a separate audit here. It is
[`../../../agentic-workflow.md`](../../../agentic-workflow.md)'s review pipeline, running
on every change:

| Stage | Runs | Against |
|---|---|---|
| Implementer agent | Writes to a human-authored specification and the capability-table row | AP-11, AP-17 |
| Reviewer agent | A fixed adversarial checklist | The contract set C-01 to C-17 |
| Verifier gate | Oracle, property, Miri, loom, fuzz, benchmark, and the registry and test-track jobs | AP-16 |
| Human sign-off | Mandatory for the low-trust tier | AP-01, AP-02, AP-03 |

Periodically, and at every increment boundary, the compliance assessment runs the
contract checks across the whole repository rather than one change
([`../phase-g-implementation-governance/compliance-assessment.md`](../phase-g-implementation-governance/compliance-assessment.md)).
The first run is recorded there; its principal finding is that the checks themselves are
not yet automated.

## 4. Escalation

1. An agent that believes a change requires breaking a contract **stops and raises it**.
   It does not implement the change and note the concern.
2. The reviewer agent flags any change touching policy, command, security, or the ingest
   gateway for human ownership rather than approving it.
3. A disagreement between the two reviewers goes to the owner.
4. A finding whose severity the engineering reviewer and the drafting agent score
   differently is recorded at the reviewer's severity, with the disagreement noted.

## 5. Dispensations

A dispensation is written permission to violate a principle for a stated period. The
form, if one is ever granted:

- Which principle and contract, and the exact scope of the exception.
- Why the compliant path was not taken.
- The expiry date or the event that ends it.
- What is done at expiry, and who does it.

**No dispensation has been granted.** Two rules are not dispensable under any
circumstance, because the product's claim rests on them: AP-01 recommend never act, and
AP-02 honest status. A change that needs either of those is a product decision, taken as
a change request under phase H, not a dispensation.

## 6. What governance costs

Roughly: one contract check per change from the reviewer agent, one full compliance
assessment per increment, and one decision log entry per architecture decision. The
tailoring in [`tailored-adm.md`](tailored-adm.md) exists to keep it at that level. If the
governance load starts to exceed the engineering load, the tailoring is wrong and is
revisited, not the principles.

## Traceability

`../../../agentic-workflow.md`; `../../../release-governance.md`;
`architecture-principles.md`; `../phase-g-implementation-governance/architecture-contracts.md`;
`../phase-h-change-management/change-management.md`;
`../../../business/risk-register.md` for the single-person risk.

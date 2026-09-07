# St-Tx Capability taxonomy

**UAF definition.** The strategic taxonomy view presents the capabilities of the
enterprise as a hierarchy.

**Purpose here.** The seven capability areas and 56 leaf capabilities Gungnir must
provide, as the strategic frame every other view traces to. Read by the owner,
customers, and plan 10 (Architecture Vision, Business Architecture).

Status: first draft, 2026-09-04. The list is plan 04's taxonomy verbatim; this view
adds the diagram and the traceability.

Diagram: [`St-Tx.puml`](St-Tx.puml) (a work-breakdown tree of the areas and leaves).

## The taxonomy

| Area | Leaves | Outcome |
|---|---|---|
| CAP-1 Sense | CAP-1.1 to CAP-1.7 | Bring every observation the sector can obtain into one governed, time-disciplined stream |
| CAP-2 Understand | CAP-2.1 to CAP-2.12 | Turn observations into one honest picture with identities, uncertainty, and context |
| CAP-3 Decide | CAP-3.1 to CAP-3.9 | Turn the picture into a policy-checked recommendation for a human |
| CAP-4 Act (recommend and authorize) | CAP-4.1 to CAP-4.7 | Put the recommendation in front of the person who holds authority, record what they decide, hand off; never act without that record |
| CAP-5 Sustain | CAP-5.1 to CAP-5.10 | Keep the sector operating, learning, and honest over time and through outages |
| CAP-6 Secure | CAP-6.1 to CAP-6.7 | Control who can see and do what, and prove it afterwards |
| CAP-7 Integrate | CAP-7.1 to CAP-7.4 | Be one node in a system of systems |

The leaf names are in `../model/elements.yaml` (section `capabilities`) and, with
statements, measures, and maturity targets, in
`../../../mission/capabilities/capability-statements.md`.

## Elements used

- CAP-1 to CAP-7 and every leaf CAP-x.y (63 registry entries).

## Notes

- The capability taxonomy is an outcome list, not a feature list; which crate
  provides each leaf is `../../../mission/capabilities/capability-to-crate-matrix.md`,
  and which are unimplemented is the gap register.
- Every leaf is exhibited by OP-30 (the C2 system) and achieved by at least one
  operational activity; the consistency check in `../tools/build_uaf.py` enforces
  both.

## Traceability

- Derives from: `../../../mission/capabilities/capability-taxonomy.md` (plan 04),
  which derives from the thread steps in `../../../mission/mission-threads.md`.
- Feeds: St-Sr, St-Cn, St-Rm; Op-Tx through `exhibits`; the capability-to-activity
  matrix; plan 05's coverage matrix; plan 10 phase B.

# St-Sr Capability structure

**UAF definition.** The strategic structure view shows how capabilities are composed
of and related to other capabilities, and which performers exhibit them.

**Purpose here.** Which performer exhibits each capability area: the C2 system for
every system capability, the roles for the capabilities in which a human acts, the
organizational nodes for what they host. Read by plan 10 (Business Architecture)
and plan 06 (which role needs which capability on screen).

Status: first draft, 2026-09-04.

## Exhibited capabilities by performer

| Performer | Exhibits (from `../model/relationships.yaml`) |
|---|---|
| OP-30 Command-and-control system | every leaf CAP-1.1 to CAP-7.4 |
| OP-31 Operator workstation | CAP-2.10, CAP-2.11, CAP-4.1, CAP-5.4, CAP-5.9, CAP-7.3 |
| OP-32 Sector service node | CAP-5.1, CAP-5.4, CAP-6.1, CAP-6.4, CAP-7.1, CAP-7.3, CAP-7.4 |
| OP-33 Sensor network | CAP-1.1, CAP-1.7 |
| OP-34 Effector interface | CAP-4.4 |
| OP-01 Operator | CAP-2.6, CAP-3.6, CAP-4.1, CAP-4.2, CAP-5.9 |
| OP-02 Supervisor | CAP-3.6, CAP-3.7, CAP-4.2, CAP-5.4, CAP-5.5, CAP-5.6, CAP-5.9 |
| OP-03 Analyst | CAP-5.2, CAP-5.3, CAP-5.7, CAP-5.9 |
| OP-04 Sensor manager | CAP-1.3, CAP-1.4, CAP-3.9, CAP-5.9 |
| OP-05 Administrator | CAP-5.6, CAP-6.1, CAP-6.2, CAP-6.3, CAP-6.5, CAP-7.3 |
| OP-06 Intelligence analyst | CAP-1.3, CAP-2.6, CAP-2.7, CAP-2.12, CAP-5.9, CAP-6.6, CAP-7.4 |
| OP-07 Planner | CAP-1.4, CAP-3.1, CAP-5.2, CAP-5.6, CAP-5.9 |
| OP-08 Commander | CAP-3.1, CAP-3.6, CAP-4.2, CAP-5.6, CAP-5.9 |

## Structure between areas

The areas form a value chain with two supporting layers:

```
CAP-1 Sense  ->  CAP-2 Understand  ->  CAP-3 Decide  ->  CAP-4 Act
        \______________ CAP-7 Integrate (peers, formats, profiles) ______________/
        \______________ CAP-5 Sustain and CAP-6 Secure (every step) ____________/
```

Within the chain, CAP-4.3 (never execute without a recorded human decision) is the
constraint every Decide and Act capability is composed under; it is not a step but a
property of the whole.

## Elements used

- OP-01 to OP-08, OP-30 to OP-34; CAP-1 to CAP-7 and their leaves.

## Notes

- A role "exhibits" a capability where the outcome depends on that role acting,
  not merely observing; the operator exhibits CAP-3.6 because their decision is part
  of enforcing the rules of engagement.
- The three adopted roles exhibit capabilities now so that plan 06 designs for them;
  their code representation is GAP-068.

## Traceability

- Derives from: St-Tx; `../../../mission/roles-and-stakeholders.md` §4 (the
  authority matrix); the `exhibits` relationships.
- Feeds: Op-Tx, Pr-Cn, the capability-to-activity matrix.

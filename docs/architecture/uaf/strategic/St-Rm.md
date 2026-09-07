# St-Rm Capability roadmap

**UAF definition.** The strategic roadmap view shows how capabilities are planned to
be delivered or changed over time.

**Purpose here.** Which capabilities reach partial and full maturity in each of the
four engineering increments, and what each increment delivers in mission terms. Read
by the owner, plan 01 (the product roadmap), and plan 10 (phases E and F).

Status: first draft, 2026-09-04. Content is plan 04's roadmap
(`../../../mission/capabilities/capability-roadmap.md`) with the D-01 and D-07
outcomes applied: everything ships in one release at the end of increment 4, and
fires (CAP-3.8) is full in increment 3.

Diagram: [`St-Rm.mmd`](St-Rm.mmd) (Mermaid Gantt; the bars show the increment in which
each area's capabilities are mostly full).

## Increments as capability deliveries

| Increment (project) | Capabilities reaching full | Capabilities reaching partial | Mission outcome |
|---|---|---|---|
| PJ-I1 Productize the core (in progress) | CAP-1.2, CAP-1.5, CAP-2.11, CAP-4.2, CAP-4.3, CAP-5.1, CAP-5.5, CAP-5.6, CAP-5.7, CAP-6.2, CAP-6.5, CAP-6.7, CAP-7.3 | 22 others | A desktop and a node that start, ingest recorded feeds, show an honest picture, journal, replay, report; nothing pretends to work |
| PJ-I2 Integrate real data | CAP-1.1, CAP-1.3, CAP-2.1, CAP-2.2, CAP-2.3, CAP-2.5 | CAP-1.6, CAP-1.7, CAP-2.4, CAP-2.9, CAP-2.12, CAP-3.1, CAP-3.9, CAP-4.5, CAP-4.6, CAP-5.8, CAP-5.10, CAP-6.1, CAP-7.4 (and the I1 partials that stay partial) | Live sensors of the lead mission through adapters; the tracking pipeline verified for Scenarios 1 to 3; cooperative identity; coverage over terrain |
| PJ-I3 Close the decision loop | CAP-1.4, CAP-1.7, CAP-2.4, CAP-2.6, CAP-2.7, CAP-2.8, CAP-2.9, CAP-2.10, CAP-3.1 to CAP-3.9, CAP-4.1, CAP-4.5, CAP-4.6, CAP-5.2, CAP-5.3, CAP-5.9, CAP-6.1 (desktop), CAP-6.3, CAP-7.2 | CAP-1.6, CAP-2.12, CAP-4.4, CAP-4.7, CAP-5.4, CAP-5.8, CAP-5.10, CAP-6.4, CAP-6.6, CAP-7.1, CAP-7.4 | The sector recommends, a human decides, it is recorded; fires included (D-07) |
| PJ-I4 Operationalize and scale | every remaining capability | none | Transport, connected profiles, failover, caller authentication, encryption, releasability, industry codecs, learned models in shadow mode, budgets met; the single release (D-01) |

## Elements used

- PJ-I1 to PJ-I4; the capabilities named; PJ-P05 (the register that orders the work).

## Notes

- The per-capability maturity table is the source; this view summarizes it. The
  generator does not derive this view because the increments are a plan, not code.
- Decisions D-01 (one release) and D-07 (fires in the first release) were applied
  on 2026-09-04; the table matches the updated plan 04 roadmap.

## Traceability

- Derives from: St-Tx, St-Cn; `../../../mission/capabilities/capability-roadmap.md`;
  `../../../gungnir-capabilities.md` §7; `../../../mission/gap-analysis/closure-roadmap.md`.
- Feeds: Pj-Rm, Sd-Rm, Pr-Rm; plan 01; plan 10 phases E and F.

# Business architecture

Status: first draft, 2026-09-04. Phase B. **The content lives in the operational,
personnel, and strategic UAF views; this document says what phase B concluded from them
and where the business gaps are.**

Domain reviewer sign-off is outstanding: the doctrine, threads, and vignettes carry the
first-draft status of plan 02 and have not been validated by a subject-matter reviewer.

## 1. Organization and roles

Eight roles, five in code and three adopted on 2026-09-04 by decision D-05.

| Role | In code | Views |
|---|---|---|
| Operator | Yes | `../../uaf/personnel/Pr-Tx.md`, `Pr-Sr.md`, `Pr-Cn.md` |
| Supervisor | Yes | as above |
| Analyst | Yes | as above |
| Sensor manager | Yes | as above |
| Administrator | Yes | as above |
| Intelligence analyst | Not yet, GAP-068 | `Pr-Tx.md`; `../../../mission/roles-and-stakeholders.md` |
| Planner | Not yet, GAP-068 | as above |
| Commander | Not yet, GAP-068 | as above |

The authority matrix, not the role list, is the specification: it says which role may
declare an identity, accept an engagement at which layer, set weapons control status,
apply a plan, promote a model, and release a product. It is enforced by GAP-058, and
until then role separation is a design intent rather than a control.

## 2. Business capabilities

Fifty-six leaf capabilities in seven groups, the taxonomy every other document indexes
on:

| Group | Leaves | Strategic view |
|---|---|---|
| CAP-1 Sense | 7 | `../../uaf/strategic/St-Tx.md` |
| CAP-2 Understand | 12 | as above |
| CAP-3 Decide | 9 | as above |
| CAP-4 Act, meaning recommend and authorize | 7 | as above |
| CAP-5 Sustain | 10 | as above |
| CAP-6 Secure | 7 | as above |
| CAP-7 Integrate | 4 | as above |

Phasing across increments is `../../uaf/strategic/St-Rm.md` and the capability roadmap.
The capability-to-crate matrix is what makes a capability a claim about the code rather
than an aspiration.

## 3. Business processes

Ten mission threads, each generated as a process view from the same source the mission
documents use, so a change to a thread step regenerates the view:

| Thread | Process | View |
|---|---|---|
| MT-01 | One-way attack drone raid against a defended-asset list | `../../uaf/operational/Op-Pr-MT-01.md` |
| MT-02 | Mixed salvo of cruise missiles, drones, and decoys | `Op-Pr-MT-02.md` |
| MT-03 | Small uncrewed aircraft over a protected site | `Op-Pr-MT-03.md` |
| MT-04 | Uncrewed surface vessel attack on a port or anchored ship | `Op-Pr-MT-04.md` |
| MT-05 | Surface picture compilation | `Op-Pr-MT-05.md` |
| MT-06 | Convoy and battery tracking with cueing of fires | `Op-Pr-MT-06.md` |
| MT-07 | Sensor management under electronic attack | `Op-Pr-MT-07.md` |
| MT-08 | Collection management and identification evidence fusion | `Op-Pr-MT-08.md` |
| MT-09 | Defended-asset planning, laydown, and rehearsal | `Op-Pr-MT-09.md` |
| MT-10 | Disconnected operation and reconnection | `Op-Pr-MT-10.md` |

The operational activities the threads decompose into are `Op-Tx.md`; who performs what
is `Op-Sr.md` and the role-to-activity matrix; what flows between performers is
`Op-Cn.md` and `Op-If.md`; the picture's state machine is `Op-St.md`.

## 4. The business gap analysis

Twenty-four of the eighty-three gaps are mission gaps, meaning the architecture does not
yet name a component responsible for part of a capability. They cluster in five places,
and the clustering is the finding:

| Cluster | Gaps | What it means |
|---|---|---|
| **Tasking and collection** | GAP-004, GAP-005, GAP-037 | The system observes sensors but cannot task them. Every collection thread stops at a recommendation the operator carries out by voice |
| **The defended-asset list** | GAP-026, GAP-027 | Threat scoring works against one protected point. Prioritization against a real asset list, with weights, does not exist, and it is the input the whole Decide group depends on |
| **Authority and escalation** | GAP-033, GAP-034, GAP-035, GAP-042 | Weapons control status, timeout, escalation, pre-delegation, and the warning function are specified in the mission documents and absent from the design |
| **The outside world** | GAP-009, GAP-040, GAP-065 | Peer ingestion, effector handoff, and coalition exchange all wait on the transport, and D-08 made each of them a generic configurable endpoint |
| **Products** | GAP-025, GAP-036, GAP-043, GAP-049, GAP-054 | Pattern of life, order of battle, fires plans, effect assessment, after-action review, and battle rhythm: the analyst and intelligence roles are the least served today |

The last cluster is worth stating plainly. The architecture is strongest where an
operator watches a live picture and weakest where an analyst reconstructs one, even
though the journal that would support the second already exists.

## 5. What phase B assumes and has not checked

- That the ten threads cover the missions a buyer cares about. Never validated with an
  operator.
- That the eight roles match how a real cell divides work, rather than how doctrine
  describes it.
- That the authority matrix is enforceable without being unusable under saturation. That
  is the tension D-15 addressed for one case, hostile uncrewed aircraft at the point
  layer, and left open elsewhere.

## Traceability

`../../uaf/operational/`, `../../uaf/personnel/`, `../../uaf/strategic/`;
`../../../mission/mission-threads.md`, `roles-and-stakeholders.md`;
`../../../mission/capabilities/capability-taxonomy.md`;
`../../../mission/gap-analysis/coverage-matrix.md`;
`../../../ux/` for the human-facing designs.

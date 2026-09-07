# Architecture definition document

Status: first draft, 2026-09-04. The consolidated architecture description: what Gungnir
is, how it is put together, what it is built on, and what is not built yet. Written to be
read straight through by someone who has never seen the project, and to hand them off to
the views and the code at every point.

If you read one document about this architecture, read this one. If you then need detail,
every section below says where it lives.

## 1. What this is

Gungnir is a command-and-control desktop application and services layer for air defence
and counter-uncrewed-aircraft operations, with maritime and land domains supporting and
intelligence and planning as cross-cutting functions. It fuses a mosaic of sensors into
one picture, scores what threatens defended assets, recommends what to do, and requires a
human to decide.

It is one node in a system of systems: several desktops may share a node, nodes exchange
with peers, and a desktop must keep working when it can reach neither.

**It is currently a verifiable scaffold, not a fielded product.** The productization
layer, the data model, and the wiring are real and tested. The tracking mathematics, the
allocator, the intercept geometry, the 3D scene, and the interface transport are trait
surfaces that return named errors and say so through health flags. Section 7 is the
honest register.

## 2. The five things that shape everything else

1. **Recommend, never act.** Nothing executes without a recorded human decision, and no
   dependency path exists by which the assistant or a model could change that.
2. **Honest status.** No flag, state, test, or screen claims a subsystem works when it
   does not.
3. **One crate set, three deployment profiles.** The disconnected desktop is the same
   software as the cloud node.
4. **One-way dependencies.** Core, model, facades, productization, deployment, interface.
   Nothing points back.
5. **Verified per capability against named oracles**, with the criteria written before the
   code.

The full set is seventeen principles in
[`preliminary/architecture-principles.md`](preliminary/architecture-principles.md), each
with a contract that checks it.

## 3. Business architecture, in summary

Eight roles: operator, supervisor, commander, analyst, intelligence analyst, sensor
manager, planner, administrator. Five are in code; three were adopted on 2026-09-04 and
are a gap. The authority matrix, not the role list, is the specification: it says who may
declare an identity, accept an engagement at which layer, set weapons control status,
apply a plan, promote a model, and release a product.

Ten mission threads describe the processes, from a drone raid against a defended-asset
list through sensor management under electronic attack to disconnected operation and
reconnection. Ten vignettes make them concrete. Fifty-six leaf capabilities in seven
groups, Sense, Understand, Decide, Act, Sustain, Secure, Integrate, index every other
document.

Detail: [`phase-b-business/business-architecture.md`](phase-b-business/business-architecture.md).
Views: the operational, personnel, and strategic domains of
[`../uaf/`](../uaf/README.md).

**The finding worth carrying forward:** the architecture is strongest where an operator
watches a live picture and weakest where an analyst reconstructs one, even though the
journal that would support the second already exists.

## 4. Data architecture, in summary

One crate owns each shared type. The primitives crate owns the track identifier, track
status, and resource identifier; the coordinate crate owns the geodetic type; the model
crate owns mission time, the canonical views, and the events. Everything above
re-exports. Nine shared types, one definition each, checked.

Provenance is a field, not a convention: source and receipt time on detections, producer
on identification evidence including a model version, lineage on identities, sequence and
mission time on envelopes.

The journal is the record. Append-only, sequenced, mission-timed, replayable to the same
picture. The node fsyncs every envelope; desktops buffer and fsync on save and every five
seconds. Journals merge in mission-time order with duplicates dropped and conflicts
reported rather than resolved.

Releasability is a property of the data, marked on views, reports, and the contract, and
enforced per caller later.

Detail: [`phase-c-information-systems/data-architecture.md`](phase-c-information-systems/data-architecture.md).
Views: the information domain.

## 5. Application architecture, in summary

Fifty crates, two of them binaries, in seven layers:

| Layer | Count | What it is |
|---|---|---|
| Tracking and estimation core | 14 | Pure numerical crates plus the three verification crates |
| Canonical model | 1 | The shared vocabulary |
| Service facades | 2 | Live tracking, intercept planning |
| Productization | 25 | Eventing, journal, configuration, session, ingest, time, interoperability, identity, identification, geospatial, analytics, policy, command, assessment, decision, model operations, security, interface, observability, resilience, collaboration, workflow, replay, reporting |
| 3D data | 2 | Loaders, GPU fusion |
| Deployment | 2 | Remote backends, the headless node |
| User interface | 4 | Rendering, viewport, panels, the desktop |

Two facades stand between the tracking core and everything above, so no application crate
depends on a filter or an allocator directly. Both speak the canonical model's types. The
graph is drawn in `ARCHITECTURE.md` from the actual manifests, and all 147 crate-to-crate
edges were checked against it today.

Two subsystems are designed and not built, each with one extra structural constraint: the
assistant, which has no state-changing tool and no path to a crate that has one; and
machine-learning inference, which produces evidence and never a decision.

Detail: [`phase-c-information-systems/application-architecture.md`](phase-c-information-systems/application-architecture.md).
Views: the services, resources, and actual-resources domains.

## 6. Technology architecture, in summary

Rust pinned to 1.98, a workspace lint policy every crate opts into, twenty pinned
dependencies all recorded in one place. Windows desktop, Linux container node.

Three deployment profiles configure one crate set: a disconnected desktop with its own
journal, an on-prem node several desktops share, and a cloud node. The profile is chosen
by configuration. Today the transport is not in the workspace, so a connected profile
reports a not-implemented transport and falls back to embedded operation with an alert,
which is principle 2 working rather than a defect.

Two graphics contexts, deliberately separate: OpenGL for the interface and viewport, a
headless compute device for point-cloud fusion, with no buffer sharing and results
crossing through CPU memory.

Fifteen standards in the standards base, five real and ten planned. Assurance is
allow-listed licences, advisory scanning, one registry for provenance, a bill of
materials, auditable binaries, keyless signatures, and a promotion step that records what
was deployed.

Detail: [`phase-d-technology/technology-architecture.md`](phase-d-technology/technology-architecture.md).
Views: the actual-resources and standards domains, plus `ARCHITECTURE.md` §8 and §9.

## 7. What is not built

Eighty-three gaps in the register, of which four were filed by the machine-learning plan
and three by this one. Grouped into eighteen work packages across three increments. The five that
matter most to a reader deciding whether to take this seriously:

| Gap | Why it dominates |
|---|---|
| GAP-011 the tracking pipeline | Everything in Understand and Decide waits on it, and it is human-owned |
| GAP-041 the transport | Every connected-profile capability waits on one stack sign-off |
| GAP-056 the performance harnesses | Every latency figure here is a budget, and nothing yet measures one |
| GAP-039 no execution without a decision | The product's central claim has no test yet |
| GAP-067 operational-readiness verification | A green core build could be mistaken for readiness |

The states at each increment boundary are in
[`phase-e-opportunities-solutions/transition-architectures.md`](phase-e-opportunities-solutions/transition-architectures.md).

## 8. How it is governed

Seventeen contracts, one per principle, applied by a reviewer agent on every change and
across the whole repository at each increment boundary. Fourteen are automatable; five
were run today and passed; two findings were raised, both about the checks not being
automatic rather than about the code.

Two contracts cannot be dispensed with under any circumstance: recommend-never-act, and
honest status. A change that needs either is a product decision, not a dispensation.

Detail: [`phase-g-implementation-governance/architecture-contracts.md`](phase-g-implementation-governance/architecture-contracts.md)
and [`compliance-assessment.md`](phase-g-implementation-governance/compliance-assessment.md).

## 9. Requirements

Fifty-eight requirements with identifiers, traced to a source, a view, a crate or gap, and
a verification gate. Thirteen are verified today, and ten of those thirteen are
constraints or data shapes rather than mission function. That ratio is the honest summary
of the state of the project.

Detail: [`requirements-management/architecture-requirements-specification.md`](requirements-management/architecture-requirements-specification.md).

## 10. Reading on

| If you want | Go to |
|---|---|
| The architecture drawn | [`../uaf/summary-and-overview.md`](../uaf/summary-and-overview.md) |
| The code's own map | `ARCHITECTURE.md` and [`../../architecture.md`](../../architecture.md) |
| What every crate does in business terms | [`../../gungnir-capabilities.md`](../../gungnir-capabilities.md) |
| What is proven and to what tolerance | [`../../verification-capability-table.md`](../../verification-capability-table.md) |
| What is missing and who owns it | [`../../mission/gap-analysis/gap-register.md`](../../mission/gap-analysis/gap-register.md) |
| The same architecture in defence framework terms | [`framework-cross-reference.md`](framework-cross-reference.md) |
| Whether it is a business | [`../../business/business-plan.md`](../../business/business-plan.md) |

## 11. What a reader should be sceptical about

Three things, stated here rather than left to be discovered:

1. **No stakeholder has been interviewed.** Roles, threads, and concerns are inferred from
   doctrine and open sources. A wrong inference propagates into the capabilities and from
   there into the gap register.
2. **No performance figure has been measured.** Every number is a budget with a scenario
   attached and no harness.
3. **The architecture description is stronger than the practice.** Fifty-eight views and a
   checked registry reflect an agent that writes quickly and a check that catches
   inconsistency, not an architecture that people have argued about.

The capability assessment scores all three honestly and does not average them away.

## Traceability

This document consolidates
[`phase-b-business/`](phase-b-business/business-architecture.md),
[`phase-c-information-systems/`](phase-c-information-systems/application-architecture.md),
and [`phase-d-technology/`](phase-d-technology/technology-architecture.md), and references
the UAF views rather than redrawing them.

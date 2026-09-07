# Plan 11: Design gap closure

## Purpose

Close the twenty-two open **design** gaps in `../mission/gap-analysis/gap-register.md`:
the capabilities where the architecture does not yet name a component responsible for
part of what the mission needs. Closing a design gap means producing the design, not the
code: the owning component, the types it introduces, the crate that owns each type, the
dependency edges it needs, the events it emits, the configuration it reads, the interface
messages it adds, the user-interface delta, and the verification row that will decide
whether the eventual implementation is correct.

The register's own vocabulary is the definition of scope. A **Mission** gap means "the
design does not cover, or covers part of, a needed capability"; a **Technical** gap means
"designed, but implementation, verification, or integration is incomplete". This plan
takes the first kind and turns each into the second, so that every remaining gap is an
engineering task with a specification behind it.

## Scope

**In scope:** the twenty-two open design gaps listed below, GAP-052 among them by finding F-1, plus one new design gap this
plan's review found (key management, under "Review findings"). Every design note produced
is complete enough to implement from: types, traits, schema, edges, events, verification
row, and interface delta.

**Out of scope:** implementation. No crate gains code under this plan. Also out of scope:
the three Mission-typed gaps that a plan already owns, GAP-044 (plan 08), GAP-046 (plan
07), and GAP-080 (plan 09); and every Technical gap, which the closure roadmap already
sequences.

### The twenty-two gaps

| Gap | Capability | Target | Owner | Design note |
|---|---|---|---|---|
| GAP-026 Defended-asset list | CAP-3.1 | I3 | Services engineer | DN-01 |
| GAP-020 Trajectory prediction and closest point of approach | CAP-2.8 | I3 | Services engineer | DN-02 |
| GAP-042 Warning function | CAP-4.5 | I3 | Services engineer | DN-03 |
| GAP-030 Effector layer and cost model | CAP-3.3 | I3 | Services engineer | DN-04 |
| GAP-036 Fires plan type and deconfliction | CAP-3.8 | I3 | Services engineer | DN-05 |
| GAP-043 Engagement tracking and effect assessment | CAP-4.6 | I3 | Services engineer | DN-06 |
| GAP-040 Effector handoff endpoint | CAP-4.4 | I4 | Services engineer | DN-07 |
| GAP-052 Policy configuration and plan validity | CAP-5.6, CAP-3.6 | I3 | Security engineer | DN-08 |
| GAP-033 Weapons control status and engagement authority | CAP-3.6 | I3 | Security engineer | DN-09 |
| GAP-034 Escalation and timeout for pending decisions | CAP-3.6, CAP-3.7 | I3 | Security engineer | DN-10 |
| GAP-004 Outbound sensor control path | CAP-1.3 | I3 | Services engineer | DN-11 |
| GAP-005 Collection requirements and tasking workflow | CAP-1.3, CAP-2.12 | I3 | Services engineer | DN-11 |
| GAP-006 Coverage-gap detection | CAP-1.4 | I3 | Services engineer | DN-12 |
| GAP-037 Sensor re-tasking recommendation | CAP-3.9 | I3 | Services engineer | DN-13 |
| GAP-017 Static hazard and barrier layer | CAP-2.5 | I3 | Services engineer | DN-14 |
| GAP-021 Track and feed anomaly detection | CAP-2.9 | I3 | Services engineer | DN-15 |
| GAP-009 Peer track and warning ingestion | CAP-1.6 | I4 | Services engineer | DN-16 |
| GAP-062 Releasability marking | CAP-6.6 | I4 | Security engineer | DN-17 |
| GAP-065 Peer and coalition exchange | CAP-7.4 | I4 | Services engineer | DN-18 |
| GAP-025 Pattern-of-life and order-of-battle products | CAP-2.12 | I4 | Services engineer | DN-19 |
| GAP-049 After-action review workflow | CAP-5.3 | I4 | Services engineer | DN-20 |
| GAP-054 Battle-rhythm support | CAP-5.8 | I4 | Services engineer | DN-21 |

Twenty-two gaps in twenty-one notes: GAP-004 and GAP-005 share DN-11 because they are the
same control path in opposite directions. GAP-084, filed by finding F-3, brings the set to
twenty-three gaps in twenty-two notes. GAP-052 appears here rather than among the
technical gaps for the reason given below.

## Review findings: four corrections to the register

Reviewing the design gaps against the coverage matrix and the code surfaced four
inconsistencies. Each is a correction the owner should approve before the design work
starts, because three of them change what this plan's scope is.

**F-1. GAP-052 is typed Technical and is a design gap.** Its own description says
identification thresholds, weapons control status, authority rules, staleness, and plan
validity "have no place in `ConfigBaseline`" — nothing is designed, which is the
register's definition of a Mission gap. It is also the highest-leverage design item in the
set after the asset list: GAP-012, GAP-018, GAP-033, GAP-035, and GAP-058 all wait on it.
**Correction:** retype to Mission and add it to this plan as DN-08.

**F-2. The CAP-6.1 coverage row is stale.** Its design coverage is recorded as partial on
the evidence that the credential mechanism is undecided. D-02 decided it on 2026-09-04:
mutual transport-layer security for machine identities, short-lived signed tokens for
operator sessions, local accounts on the disconnected desktop. **Correction:** design
coverage becomes full and the evidence cites D-02; GAP-057 remains as the implementation
gap.

**F-3. CAP-6.4 has no design gap and is genuinely undesigned.** Its coverage evidence says
"no component or key management designed", but its only gap, GAP-060, is typed Technical
and its closing action assumes a design that does not exist. Encryption in transit follows
from D-02; key custody, rotation, and escrow across three profiles have no owning
component. **Correction:** file GAP-084, a design gap for key management, and add DN-22.

**F-4. The CAP-5.6 row omits the asset list.** Its evidence names three things missing
from the baseline schema — the asset list, policy configuration, and plan validity — but
lists only GAP-071, GAP-051, and GAP-052. GAP-026 carries the asset list. **Correction:**
cross-list GAP-026 on the CAP-5.6 row.

One judgement call was left alone rather than corrected. CAP-6.2's per-class and per-layer
authorization refinements sit under GAP-058, typed Technical. The authority matrix in
`../mission/roles-and-stakeholders.md` §4 is a specification, so implementation is the
honest label. If the engineering reviewer disagrees it becomes a twenty-second design
note.

## Inputs

- `../mission/gap-analysis/gap-register.md` and `coverage-matrix.md` for the gaps and the
  coverage assessment they close.
- `../mission/capabilities/capability-statements.md` for what each capability must do,
  and `../mission/measures.md` for the measure each design must satisfy.
- `../mission/mission-threads.md` and `vignettes.md` for the thread step each closure
  serves.
- `../../ARCHITECTURE.md` §7.1 for the dependency graph every new edge must join, §8 for
  the profiles every design must work in, and §10 for the open-items ledger.
- `../architecture/togaf/preliminary/architecture-principles.md` and
  `../architecture/togaf/phase-g-implementation-governance/architecture-contracts.md` for
  the rules each design must obey.
- `../architecture/uaf/model/elements.yaml` for the identifiers each design references.
- `../gungnir-api-v1.md` for the interface contract three closures extend.
- `../verification-capability-table.md` §2 for the rows each closure adds.
- `../ux/information-architecture.md` and `wireframes/` for the panels each closure
  touches.

## Deliverables and target location

All under `docs/design/`:

| Path | Content |
|---|---|
| `README.md` | Index, the design-note format, the rule that a design note is complete only when its verification row is written |
| `dependency-edges.md` | Every new edge this plan adds, its justification against AP-10, the acyclicity check, and the `ARCHITECTURE.md` §7.1 delta as one reviewable set |
| `model-and-schema-deltas.md` | Every new type in `gungnir-model` and every new section in the configuration baseline, consolidated so the canonical model is reviewed once rather than twenty-two times |
| `verification-rows.md` | The new `verification-capability-table.md` §2 rows, written before any code exists (AP-17) |
| `DN-01-defended-assets.md` | GAP-026. The asset list: types, priority semantics, warning obligations, the baseline section, and how assessment scores against a list rather than a point |
| `DN-02-prediction-and-approach.md` | GAP-020. Predicted path, time to impact per asset, closest point of approach |
| `DN-03-warning.md` | GAP-042. The warning rule, its lead-time threshold, its channel, and its record |
| `DN-04-effector-model.md` | GAP-030. Layer, unit cost, magazine, and reserve on the resource config and view |
| `DN-05-fires.md` | GAP-036. The fires plan variant, deconfliction rules, and the fires handoff message |
| `DN-06-engagement-and-effect.md` | GAP-043. Engagement state keyed by decision, outcome events, and re-engagement |
| `DN-07-handoff.md` | GAP-040. The handoff message in the v1 contract, carrying the decision record and track provenance |
| `DN-08-policy-configuration.md` | GAP-052 (retyped, finding F-1). The policy section and validity window in the baseline schema |
| `DN-09-authority-and-control-status.md` | GAP-033. Weapons control status per layer and engagement authority by role and class. **Human-owned** |
| `DN-10-queue-expiry-and-escalation.md` | GAP-034. Expiry, escalation, and the two new decision kinds. **Human-owned** |
| `DN-11-sensor-control-and-tasking.md` | GAP-004, GAP-005. The outbound control message, the acknowledgement event, the requirement type, and the tasking case |
| `DN-12-coverage-and-gaps.md` | GAP-006. The combined-coverage and gap query over the sensor registry |
| `DN-13-sensor-retasking.md` | GAP-037. The sensor-plan recommendation with before and after coverage as its rationale |
| `DN-14-hazard-layer.md` | GAP-017. The hazard and barrier layer type and how it is drawn |
| `DN-15-anomaly-detectors.md` | GAP-021. The detector set, per D-13's pure-function pattern |
| `DN-16-peer-sources.md` | GAP-009. The peer-source adapter, its provenance, its staleness, and its configurable quality |
| `DN-17-releasability.md` | GAP-062. The marking on views, reports, and the contract, and enforcement per caller. **Human-owned** |
| `DN-18-coalition-exchange.md` | GAP-065. The composition of inbound peers, outbound stream and reports, and marking |
| `DN-19-order-of-battle.md` | GAP-025. Cross-session queries and the versioned order-of-battle product |
| `DN-20-after-action-review.md` | GAP-049. The review case type linked to a replayed session and its report |
| `DN-21-battle-rhythm.md` | GAP-054. Scheduled products and the maintenance-window state |
| `DN-22-key-management.md` | GAP-084 (new, finding F-3). The owning component for key custody and rotation across the three profiles. **Human-owned** |

Twenty-six documents. Each design note also lands its changes in the document that owns
the fact, in the same change: `ARCHITECTURE.md` for an edge, the capability statement for
a capability's provider list, `gungnir-api-v1.md` for a message, the verification table
for a row, and the coverage matrix for the row that moves to full.

## Deliverable outline

Every design note has the same eight sections, in this order:

1. **The gap and the thread step it blocks.** One paragraph naming the mission thread
   step that stops today, so the design is anchored to a mission need and not to an
   architectural preference.
2. **The owning component.** Which crate, and why that crate under AP-06 and AP-10.
3. **Types.** Rust sketches for the types introduced, naming the owning crate for each,
   with re-exports rather than redefinitions.
4. **Edges.** Any new dependency edge, its justification, and its entry in
   `dependency-edges.md`. A note that needs no edge says so explicitly.
5. **Behaviour.** The trait or function surface, what it reads, what it emits, and what
   it does when its inputs are missing or stale, which is where AP-02 is either honoured
   or quietly broken.
6. **Configuration and interface delta.** New baseline sections and new contract
   messages, with version consequences.
7. **User-interface delta.** Which panel changes, referencing the plan 06 identifier; and
   how absence is shown.
8. **Verification.** The `verification-capability-table.md` §2 row: capability, method,
   pass criterion, and data source, with the criterion agreed before implementation.

A note without section 8 is not finished. That rule exists because AP-17 is the principle
most likely to erode under delivery pressure, and a criterion written after the code is
written to fit the code.

## Method

1. **Correct the register first**, per findings F-1 to F-4. Retype GAP-052, refresh the CAP-6.1 coverage row,
   file GAP-084, and cross-list GAP-026 on the CAP-5.6 row. Regenerate the register so
   the counts in every ledger agree before any design is written against them.
2. **Design the keystone.** DN-01 defended assets, then DN-04 effector model, then DN-08
   policy configuration. Six later notes read from these three, so an error here
   propagates further than anywhere else in the set.
3. **Design the decision loop.** DN-02, DN-03, DN-05, DN-06, DN-09, DN-10.
4. **Design sensing and the picture.** DN-11 to DN-15.
5. **Design the outward-facing set.** DN-07, DN-16, DN-17, DN-18.
6. **Design the analyst products.** DN-19, DN-20, DN-21.
7. **Design key management** (DN-22), which is human-owned and gates nothing else, so it
   runs in parallel with the owner rather than blocking the sequence.
8. **Consolidate.** Write `dependency-edges.md`, `model-and-schema-deltas.md`, and
   `verification-rows.md` from the notes, then run the acceptance checks.
9. **Land the in-place updates** and regenerate the register and the coverage matrix.

## Dependency edges: the rule this plan works under

Decided 2026-09-05 by the owner: **new dependency edges are allowed where the coupling is
natural**, each drawn in `ARCHITECTURE.md` §7.1 in the same change that introduces it.
The alternative considered and not taken was to extend D-13's pure-function pattern to
every closure.

Consequences the plan carries deliberately:

- **D-13 still stands.** The anomaly detectors remain pure functions in
  `gungnir-analytics` with no new edge, because that decision is recorded and this plan
  does not reopen resolved decisions. DN-15 follows it.
- **Every edge is argued, not assumed.** A design note that adds an edge states what it
  would otherwise have to duplicate, and the reviewer may reject the edge and require the
  data to be passed in instead.
- **The acyclicity check runs on every candidate.** All six edges identified during this
  review are acyclic against the current graph, verified on 2026-09-05.

| Candidate edge | Serves | Acyclic | Note |
|---|---|---|---|
| `gungnir-analytics` to `gungnir-sensor-management` | DN-12 coverage query | Yes | Makes analytics central; the reviewer should weigh that |
| `gungnir-decision` to `gungnir-analytics` | DN-13 re-tasking | Yes | |
| `gungnir-workflow` to `gungnir-assessment` | DN-03 warning | Yes | |
| `gungnir-workflow` to `gungnir-sensor-management` | DN-11 tasking case | Yes | May be avoidable by linking on identifier |
| `gungnir-reporting` to `gungnir-identity` | DN-19 order of battle | Yes | |
| `gungnir-assessment` to `gungnir-config` | DN-01 asset list | Yes | **Not recommended.** The asset view belongs in `gungnir-model` under AP-06, which removes the need for the edge |

One case cannot take an edge under any justification: DN-06 keys engagement state by
decision, and `gungnir-intercept-service` is a service facade while `gungnir-command` is a
productization crate. A facade may not depend on productization (AP-10), so the design
uses the decision identifier the canonical model already owns.

## Roles

- **Owner (architecture board):** every dependency edge; the human-owned notes DN-09,
  DN-10, DN-17, DN-22; the register corrections F-1 to F-4; any new decision the pass raises.
- **Design agent:** the notes, the consolidations, the in-place updates, the checks.
- **Engineering reviewer:** the type placements, the edges, and the effort consequences.
- **Domain reviewer:** that each note serves the thread step it claims, which is the one
  thing an agent cannot check.

## Dependencies

Plans 02, 04, and 05 for the mission content and the register; plan 03 for the element
registry each note references; plan 06 for the panel identifiers; plan 10 for the
principles and contracts each design must obey. No engineering work depends on this plan
finishing, but every increment-3 work package in
`../architecture/togaf/phase-e-opportunities-solutions/work-packages.md` is specified by
it.

## Effort and sequencing

Ten to sixteen agent-assisted days. The human-owned share is four notes plus every
dependency edge, which is where the elapsed time will actually go.

Sequencing is the method order above, and it is not arbitrary: three notes gate six, so
the keystone runs alone before the rest fan out.

## Acceptance criteria

1. Every one of the twenty-three gaps has a design note naming the owning component, its
   types, its edges, its behaviour under missing input, and its verification row.
2. Every coverage-matrix row whose design coverage was partial or none moves to full, or
   the note states plainly why it cannot yet.
3. Every new dependency edge is drawn in `ARCHITECTURE.md` §7.1 in the same change, and
   the acyclicity check passes.
4. Every new shared type has exactly one owning crate, checked the way the compliance
   assessment checks the existing nine.
5. Every closure has a `verification-capability-table.md` §2 row whose criterion was
   agreed before implementation started.
6. No note introduces a path by which anything acts without a recorded human decision
   (C-01), and no note introduces a health flag or a default that could report unearned
   readiness (C-02).
7. The register and the coverage matrix are regenerated, and the gap counts in
   `ARCHITECTURE.md` §10, `docs/README.md`, and `docs/plans/README.md` agree.

Criterion 6 is checked by a human, not by the drafting agent, on every note that touches
policy, command, security, or the ingest gateway.

## Execution outcome, 2026-09-05

| Criterion | State |
|---|---|
| 1. Every gap has a design note with component, types, edges, behaviour, verification row | Met. 22 notes, 23 gaps |
| 2. Coverage rows move to full or say why not | Met. 22 rows moved; CAP-6.2 stays partial as the plan predicted, and CAP-5.2 stays partial under plan 07's gap |
| 3. Every new edge drawn in `ARCHITECTURE.md` §7.1 and acyclic | Met as far as design can meet it. All five checked acyclic, recorded, and **accepted by the engineering reviewer on 2026-09-05**; each is drawn as it enters a manifest |
| 4. Every new shared type has one owning crate | Assigned in `../design/model-and-schema-deltas.md`; checkable only when the code exists |
| 5. Every closure has an agreed verification row | **Met 2026-09-05.** 23 rows agreed by the owner and promoted into `../verification-capability-table.md` §2 |
| 6. No note introduces a path to act without a decision, or an unearned health flag | Met. Held by construction across all 22, DN-10 states that no configuration can make an expiry accept, and a domain reviewer checked each note against its thread step on 2026-09-05 |
| 7. Register regenerated and counts agree everywhere | Met |

All five human-owned notes are signed: DN-08, DN-09, DN-10, DN-17, DN-22. Both reviews
are complete as of 2026-09-05. Every criterion is met except 4, which waits on code: the
single-owning-crate check can only run once the types exist, and GAP-081 automates it.

## Risks

| Risk | Mitigation |
|---|---|
| The asset list shape is wrong and six closures inherit the error | It is designed first, alone, against MOP-27 and the CAP-3.1 statement, and reviewed before anything reads it |
| Allowing edges increases coupling in the productization layer | Per-edge justification, the acyclicity check, and the reviewer's right to require the data be passed in instead |
| Designing twenty-two closures without a domain reviewer designs the wrong thing well | Each note names the thread step it serves, so a wrong design is visible as a mismatch rather than as a plausible document |
| Design without implementation drifts from the code | The verification row is written at design time and becomes the contract the implementation is measured against |
| The pass raises more decisions than it closes | Expected. New decisions take D-nn identifiers in `decisions-needed.md` and do not block the notes that do not need them |
| Twenty-six new documents add weight to a set that is already large | Three of them are consolidations that exist so the canonical model, the edges, and the verification rows are each reviewed once |

## Open questions

- **Escalation and expiry values.** DN-10 needs a timeout per engagement layer and per
  weapons control status. D-16 agreed the measure targets but not these. Expect a new
  decision.
- **Warning lead time.** DN-03 needs a threshold per asset class and per channel. The
  design can express it as configuration, but the default values are the owner's.
- **Unit cost semantics.** MOE-03 is defined by effector layer, not by money, so DN-04
  makes layer mandatory and unit cost an optional relative figure used only for
  tie-breaking. Recorded here as a design call rather than a question, and open to
  challenge.
- **Key custody in the cloud profile.** DN-22 says keys are off-host; whose host is a
  deployment question with a contractual answer, not an architectural one.
- **Whether GAP-058 is really technical.** The per-class and per-layer authorization
  refinements have a specification in the authority matrix, so this plan treats them as
  implementation. If the reviewer disagrees, it becomes a twenty-second design note.

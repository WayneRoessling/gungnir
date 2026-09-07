# Architecture requirements specification

Status: first draft, 2026-09-04. Requirements management. Fifty-eight requirements with
identifiers, each traced to a source, to a view, to the code or gap that carries it, and
to how it is verified.

**What is new here is the identifier, not the requirement.** Every statement below was
already implied by a capability, a measure, a decision, or a standard. Giving each one a
stable identifier is what allows a test, a view, and a code path to point at the same
thing, which is TOGAF's requirements-management discipline and the one thing the UAF
registry did not yet carry.

Priority column: the increment the requirement must hold by. Verification column: the
gate or measure that decides it.

## 1. Functional requirements

| Id | Requirement | Source | View | Carried by | Priority | Verification |
|---|---|---|---|---|---|---|
| REQ-F-01 | Observations from heterogeneous sensors are ingested through one governed gateway that validates and quarantines | CAP-1.1, CAP-1.2 | `Op-Pr-MT-01`, `Rs-Cn` | `gungnir-ingest`; GAP-001, GAP-064 | I2 | Verification table, ingest row |
| REQ-F-02 | Every source is authenticated before its data enters the picture | CAP-1.2, D-02 | `Sc-Pr` | `gungnir-security`; GAP-002 | I2 | Security gate |
| REQ-F-03 | Sensor modes, tasking, and calibration are held in a registry and changeable by the sensor manager | CAP-1.3 | `Rs-Cn`, `Op-Pr-MT-07` | `gungnir-sensor-management`; GAP-003, GAP-004 | I2 for the registry, I3 for outbound control | Verification table, sensor-management row |
| REQ-F-04 | Coverage is computed and coverage gaps are detected and displayed | CAP-1.4 | `Op-Pr-MT-07` | `gungnir-analytics`; GAP-006, GAP-007 | I3 | MOP for coverage; usability round 2 |
| REQ-F-05 | Data from sources with different clocks is time-disciplined and skew is detected | CAP-1.5 | `If-Cn` | `gungnir-time`; GAP-008 | I2 | MOP-09; the test-track skew case |
| REQ-F-06 | Cooperative identity from open protocols is decoded and fused as evidence | CAP-1.7, D-09 | `If-Tx` | `gungnir-interop`, `gungnir-identification`; GAP-010 | I2 | Codec conformance |
| REQ-F-07 | A multi-sensor, multi-rate, out-of-sequence tracking pipeline maintains the picture | CAP-2.1 to CAP-2.5 | `Op-St`, `Rs-Pr` | The tracking core; GAP-011, GAP-013, GAP-015, GAP-016 | I2, random finite sets I3 | Verification table §1, oracle agreement |
| REQ-F-08 | Tracks carry a quality and a staleness that the interface renders differently from fresh data | CAP-2.2 | `Op-St` | `gungnir-model`; GAP-012 | I3 | Usability round 2; C-02 |
| REQ-F-09 | Classification and identification produce evidence with a margin rule and per-class thresholds set by policy | CAP-2.6 | `If-Tx` | `gungnir-identification`, `gungnir-policy`; GAP-018 | I3 | MOE for identification; ML-01 gates |
| REQ-F-10 | Entities keep a global identity with lineage across sources and sessions | CAP-2.7 | `If-Sr` | `gungnir-identity`; GAP-019, GAP-069 | I3 | Property test on lineage |
| REQ-F-11 | Threats are scored against a defended-asset list with per-class lethality and asset weighting | CAP-3.1, CAP-3.2 | `Op-Pr-MT-01` | `gungnir-assessment`; GAP-026, GAP-027 | I3 | MOE-01; scenario replay |
| REQ-F-12 | The system recommends a cheapest-adequate assignment of resources to tracks within readiness, geometry, and policy | CAP-3.3, CAP-3.4 | `Sv-Pr` | `gungnir-allocation`, `gungnir-intercept-service`; GAP-029, GAP-030, GAP-031 | I3 | Verification table, allocation row |
| REQ-F-13 | Every recommendation carries a rationale and its alternatives | CAP-3.5 | `Op-Is-VG-01` | `gungnir-decision`; GAP-032 | I3 | Usability round 2; assistant evaluation |
| REQ-F-14 | The queue orders pending decisions by priority and time remaining, and supports pre-delegation where policy allows | CAP-3.7, D-15 | `Op-Pr-MT-02` | `gungnir-command`; GAP-034, GAP-035 | I3 | MOP for time to decision |
| REQ-F-15 | A session is journaled, replayable, and reproduces the same picture | CAP-5.2 | `Rs-St` | `gungnir-store`, `gungnir-replay`; GAP-045 | I2 | Journal round-trip; replay determinism |
| REQ-F-16 | Measures are computed from the journal without manual collation | CAP-5.3 | `Pm-Me` | `gungnir-reporting`; GAP-047 | I3 | The measures themselves |

## 2. Data requirements

| Id | Requirement | Source | View | Carried by | Priority | Verification |
|---|---|---|---|---|---|---|
| REQ-D-01 | Shared types have exactly one owning crate; everything above re-exports | AP-06 | `If-Sr` | `gungnir-core`, `gungnir-coord`, `gungnir-model` | Now | Contract C-07, checked |
| REQ-D-02 | Every detection carries its source and its receipt time | AP-07, CAP-1.2 | `If-Tx` | `gungnir-model` | Now | Schema review; C-08 |
| REQ-D-03 | Identification evidence names its producer, including model name and version for a machine-learning source | AP-07, ML-01 | `If-Tx` | `gungnir-identification`; GAP-080 | I4 | ML promotion gate |
| REQ-D-04 | Envelopes carry a bus sequence number and a mission time | AP-08 | `If-Cn` | `gungnir-eventing` | Now | Ordering and fan-out tests |
| REQ-D-05 | The node journal fsyncs every envelope; desktop journals fsync on session save and every five seconds | D-04 | `Rs-St` | `gungnir-store` | I2 | Journal durability budget |
| REQ-D-06 | Journals merge in mission-time order, duplicates are dropped, and conflicting decisions are reported rather than resolved | D-03 | `Rs-St` | `gungnir-resilience`, `gungnir-collab` | I4 | Reconciliation invariant tests |
| REQ-D-07 | Views, reports, and the interface contract carry a releasability marking | D-06, CAP-6.6 | `If-Tx` | `gungnir-model`, `gungnir-api`; GAP-062 | I3 marking, I4 enforcement | Schema review; per-caller test |
| REQ-D-08 | Training data and model artefacts live outside this repository; only the manifest enters it | ML plan | not modelled | `gungnir-modelops`; GAP-078 | I4 | Manifest schema; repository policy |

## 3. Interoperability requirements

| Id | Requirement | Source | View | Carried by | Priority | Verification |
|---|---|---|---|---|---|---|
| REQ-I-01 | The node exposes a versioned contract for snapshot, event stream, detection submission, and plan decisions | CAP-7.1 | `Sv-Cn` | `gungnir-api`; GAP-041 | I4 | Conformance suite, GAP-063 |
| REQ-I-02 | Schema versions are negotiated explicitly and an incompatible peer is refused, not silently accepted | CAP-7.2 | `Sd-Tx` | `gungnir-interop` | Now | Catalogue tests |
| REQ-I-03 | Radar feeds decode from ASTERIX category 048 | SD-03, D-09 | `Sd-Tx` | `gungnir-interop`; GAP-064 | I2 | Codec conformance, fuzz |
| REQ-I-04 | Coalition track exchange uses STANAG 4676 | SD-04, D-06 | `Sd-Tx` | `gungnir-interop`; GAP-064, GAP-065 | I4 | Codec conformance |
| REQ-I-05 | Each external party is a configurable endpoint with a message type rather than a bespoke integration | D-08 | `Op-Cn` | `gungnir-api`; GAP-009, GAP-040, GAP-042 | I4 | Synthetic peer tests |
| REQ-I-06 | Detections are exportable in a columnar format for analytics and bulk exchange | SD-02 | `Sd-Tx` | `gungnir-interop` | Now | Round-trip test |

## 4. Security requirements

| Id | Requirement | Source | View | Carried by | Priority | Verification |
|---|---|---|---|---|---|---|
| REQ-S-01 | No engagement, effector command, or reserved identity declaration executes without a recorded human decision | AP-01, CAP-4.3 | `Sc-Pr` | `gungnir-policy`, `gungnir-command`; GAP-039 | I3 | Contract C-01; a dedicated test |
| REQ-S-02 | Authority is enforced per role, per class, and per engagement layer, per the authority matrix | CAP-6.2, D-05 | `Sc-Sr` | `gungnir-security`; GAP-058, GAP-068 | I3 | Authorization tests |
| REQ-S-03 | Weapons control status is set by the authorized roles and constrains every recommendation | CAP-3.6 | `Sc-Pr` | `gungnir-policy`; GAP-033 | I3 | Policy tests |
| REQ-S-04 | Pending decisions escalate and time out rather than waiting indefinitely | CAP-3.6, CAP-3.7 | `Op-Pr-MT-02` | `gungnir-command`; GAP-034 | I3 | Queue tests |
| REQ-S-05 | Every consequential act is attributable to a person, a role, and a mission time in an audit log | AP-03, CAP-6.3 | `Sc-Pr` | `gungnir-security`; GAP-059 | I3 | Audit completeness test |
| REQ-S-06 | Machine identities authenticate with mutual transport-layer security; operator sessions use short-lived signed tokens | D-02 | `Sc-Cn` | `gungnir-security`; GAP-057 | I4 | Security gate |
| REQ-S-07 | Data is encrypted in transit on every network crossing and at rest where the deployment requires it | CAP-6.4 | `Sc-Cn` | GAP-060 | I4 | Security gate |
| REQ-S-08 | The assistant has no tool that changes state and no dependency path to a crate that does | AI plan | `Sc-Tx` | `gungnir-agent`; GAP-044 | I4 | Dependency-list test; injection cases |
| REQ-S-09 | Untrusted text from sensors, peers, annotations, and reports is treated as data by the assistant, never as instruction | AI plan | `Sc-Tx` | GAP-044 | I4 | Seven injection cases, every case every run |
| REQ-S-10 | Every release carries a bill of materials, a signature, and its assurance reports | CAP-6.5 | `Sd-Rm` | Release workflow; GAP-061 | I2 | The workflow itself |

## 5. Performance requirements

Values are the confirmed provisional gates from
[`../../../performance-budgets.md`](../../../performance-budgets.md); the harnesses are
GAP-056. The requirement is the budget, not a restatement of the number, so a change to
the budget changes one place.

| Id | Requirement | Source | Priority | Verification |
|---|---|---|---|---|
| REQ-P-01 | The desktop sustains its frame-rate budget with the 3D viewport open under a dense-swarm scenario | Budgets, desktop | I3 | Tick harness plus viewport spans |
| REQ-P-02 | The per-frame tick stays inside its budget with ingest, poll, plan, and journal all running | Budgets, desktop | I2 | Tick harness |
| REQ-P-03 | Snapshot calls do not touch the pipeline and stay inside their budget | Budgets, desktop | I2 | Tick harness |
| REQ-P-04 | Detection to on-screen update stays inside its end-to-end budget in the embedded profile | Budgets, desktop | I2 | Scenario 3 replay |
| REQ-P-05 | The node sustains its detection ingest throughput through validation and quarantine | Budgets, node | I4 | Load generation through the recorded adapter |
| REQ-P-06 | Detection to event-stream publish stays inside its budget on-prem and in cloud | Budgets, node | I4 | Node tick harness |
| REQ-P-07 | An accepted envelope is durable within its budget, and the node recovers to serving within its budget after restart | Budgets, node | I4 | Checkpoint replay test |
| REQ-P-08 | A desktop falls back to embedded operation within its budget after link loss, stores and forwards to capacity, and reconciles within its budget | Budgets, connectivity | I4 | Integration test that cuts the transport |
| REQ-P-09 | Covariances stay positive semi-definite and no value becomes non-finite over long runs | Verification table §1 | Now | Property tests, already a gate |

## 6. Usability requirements

| Id | Requirement | Source | View | Carried by | Priority | Verification |
|---|---|---|---|---|---|---|
| REQ-U-01 | Each role has a workspace containing what that role's threads need and not more | CAP-5.9, D-05 | `Pr-Cn` | GAP-055 | I3 | Usability round 2 |
| REQ-U-02 | Every layout carries a status strip stating what the system knows and does not know | UX plan | `Pr-Cn` | GAP-072 | I3 | Heuristic walkthrough; MOP-37 |
| REQ-U-03 | Absence, staleness, and degradation are visible rather than inferred from silence | AP-02 | `Op-St` | GAP-072, GAP-073 | I3 | Usability round 2; contract C-02 |
| REQ-U-04 | Machine assistance is labelled, sourced, and carries no authority in the interface | AI plan, UX principle 7 | not modelled | GAP-044 | I4 | Assistant evaluation; usability round 2 |
| REQ-U-05 | Default display vocabulary is the agreed standard terms, with a per-deployment override table | D-12 | not modelled | GAP-070 | I3 | Baseline schema test |

## 7. Constraint requirements

These constrain how the architecture may be built rather than what it does. Each is a
principle expressed as a testable statement.

| Id | Requirement | Principle | Verification |
|---|---|---|---|
| REQ-C-01 | No dependency edge exists that `ARCHITECTURE.md` does not draw | AP-10 | Contract C-11, checked |
| REQ-C-02 | Every dependency is recorded in the standards document §2.9 and pinned once | AP-14 | Contract C-14, checked |
| REQ-C-03 | No `unwrap()` or `expect()` outside tests, entry points, and debug-only invariant checks | AP-12 | Checked, zero today |
| REQ-C-04 | Unimplemented capability returns a named error on every reachable path | AP-12 | Contract C-03; GAP-082 |
| REQ-C-05 | The graphics and compute contexts stay separate and separately pinned | AP-15 | Contract C-15 |
| REQ-C-06 | Deployment profiles differ by configuration and wiring, never by conditional logic in a domain crate | AP-05 | Contract C-06 |
| REQ-C-07 | The two binaries contain wiring, not domain logic | AP-13 | Contract C-13 |
| REQ-C-08 | No pass criterion or measure target is widened to make a test pass | AP-17 | Contract C-17; diff review |
| REQ-C-09 | Every artefact is unclassified and derived from public sources | AP-04 | Contract C-05 |
| REQ-C-10 | No verification gate is waived by an agent | AP-16 | Contract C-16 |

## 8. Coverage

| Category | Count | Verified today | Waiting on a gap |
|---|---|---|---|
| Functional | 16 | 2 partly | 14 |
| Data | 8 | 4 | 4 |
| Interoperability | 6 | 2 | 4 |
| Security | 10 | 0 | 10 |
| Performance | 9 | 1 | 8 |
| Usability | 5 | 0 | 5 |
| Constraint | 10 | 4 checked today | 6 |
| **Total** | **64** | **13** | **45** |

Thirteen of fifty-eight are verified today, and ten of those thirteen are constraints or
data-shape requirements rather than mission function. That ratio is the honest summary of
the project's state and matches what the transition architectures say about T1.

The total read 58 until 2026-09-06; the tables above hold 64 (16 F, 8 D, 6 I, 10 S, 9 P,
5 U, 10 C), counted by `docs/architecture/uaf/tools/build_uaf.py`, which generates the
registry's requirements from these tables (GAP-083).

## 9. What has no requirement here

Deliberately: anything about autonomous engagement, anything about a specific customer's
accreditation regime, and anything about the commercial product beyond what the
architecture must support. The business requirements live in the business plan and are
not architecture requirements.

## Traceability

Requirements are stored and traced per
[`requirements-repository.md`](requirements-repository.md). Sources:
`../../../mission/capabilities/capability-statements.md`;
`../../../mission/measures.md`; `../../../performance-budgets.md`;
`../../uaf/standards/Sd-Tx.md`; `../../../mission/gap-analysis/decisions-needed.md`;
`../preliminary/architecture-principles.md`.

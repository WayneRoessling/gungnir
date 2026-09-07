# Coverage matrix

Status: first draft, 2026-09-04. For each leaf capability: **design coverage** (does the
architecture name a component responsible for every part of it: full, partial,
none, or planned when a plan in `../../plans/` owns it) and **implementation
coverage** (from `../capabilities/capability-to-crate-matrix.md`: full, partial,
none), with the evidence and the gaps that follow. A capability with full design
coverage and partial implementation has technical gaps only; anything less than
full design coverage has at least one mission gap.

Twenty-two rows reached full design coverage on 2026-09-05 through plan 11's
design notes in `../../design/`. That set is signed and reviewed: the owner signed
all five human-owned notes, the engineering reviewer accepted the five new
dependency edges, and a domain reviewer checked each note against the mission
thread step it names. Full design coverage means a reviewed component is named for
every part of the capability; implementation is what remains.

| Capability | Design | Implementation | Evidence | Gaps |
|---|---|---|---|---|
| CAP-1.1 Ingest observations | full | partial | `gungnir-ingest` gateway and adapter trait; `gungnir-interop` codec boundary; `DetectionView` carries provenance | GAP-001, GAP-064 |
| CAP-1.2 Validate and quarantine | full | partial | `IngestGateway` validation and quarantine; `gungnir-security` authentication trait | GAP-002 |
| CAP-1.3 Sensor modes and tasking | full | partial | Registry, modes, coverage regions, and the outbound control path are built as of 2026-09-05 (GAP-003, GAP-004): a command is recorded, published, and swept for acknowledgement, and a requested mode is kept apart from the confirmed one. No adapter carries a command to a real sensor (GAP-001) and collection requirements are built and tasked from PN-15 but the list is not rebuilt at startup (GAP-005); DN-11 (`../../design/`) names a component for every remaining part | GAP-003, GAP-004, GAP-005 |
| CAP-1.4 Coverage and gaps | full | partial | Coverage volumes in `gungnir-analytics`; gap detection as a function and map rendering of coverage are not designed; a component is now named for every part of it by DN-12 (`../../design/`, first draft 2026-09-05) | GAP-006, GAP-007 |
| CAP-1.5 Time discipline | full | partial | `gungnir-time` clocks and late-data policy; `DetectionView` source and receipt time | GAP-008 |
| CAP-1.6 Peer early warning | full | none | API snapshot and event stream exist for peers; no designed path that merges peer tracks as a source with visible staleness; a component is now named for every part of it by DN-16 (`../../design/`, first draft 2026-09-05) | GAP-009, GAP-091 |
| CAP-1.7 Cooperative identity | full | none | Codec boundary in `gungnir-interop`; evidence model in `gungnir-identification` | GAP-010 |
| CAP-2.1 Multi-sensor picture | full | partial | Tracking core trait surfaces, `gungnir-fusion-async`, `gungnir-tracking-service` facade | GAP-011, GAP-016, GAP-048, GAP-066 |
| CAP-2.2 Tracks through gaps | full | partial | `Quality` on `TrackView`; stale drawn muted; stale never allocated | GAP-011, GAP-012 |
| CAP-2.3 Sensor registration | full | none | `gungnir-track-fusion` registration traits; integration with `gungnir-data-fusion` flagged as a risk in `../../gungnir-capabilities.md` §9 | GAP-013, GAP-014 |
| CAP-2.4 Dense groups | full | none | `gungnir-rfs` PHD/CPHD and LMB traits | GAP-015 |
| CAP-2.5 Clutter-tolerant surface picture | full | none | Scenario 2 and its verification rows; no static hazard or barrier layer in `gungnir-geo`; a component is now named for every part of it by DN-14 (`../../design/`, first draft 2026-09-05) | GAP-011, GAP-016, GAP-017 |
| CAP-2.6 Classify and identify | full | partial | `gungnir-identification` engine with margin rule; per-class thresholds belong to `gungnir-policy`; plan 09 ML-01 | GAP-018, GAP-073, GAP-077, GAP-080 |
| CAP-2.7 Global identity | full | partial | `gungnir-identity` resolver with lineage; cross-session correlation is an `../../../ARCHITECTURE.md` §10 item | GAP-019, GAP-069 |
| CAP-2.8 Predict trajectory and approach | full | partial | `gungnir-assessment` scores closing speed to one point; trajectory prediction and closest point of approach are not designed; a component is now named for every part of it by DN-02 (`../../design/`, first draft 2026-09-05) | GAP-020 |
| CAP-2.9 Anomalies | full | none | Alerts and correlation exist in `gungnir-observability`; no designated home for anomaly rules; plan 09 ML-04 for learned detection; a component is now named for every part of it by DN-15 (`../../design/`, first draft 2026-09-05) | GAP-021, GAP-080 |
| CAP-2.10 Terrain and map context | full | partial | `gungnir-data` loaders, `gungnir-data-fusion`, `gungnir-geo` layers, `gungnir-viewport3d` scene | GAP-014, GAP-022, GAP-023, GAP-024 |
| CAP-2.11 Geometric questions | full | full | `gungnir-analytics` implemented and tested | none |
| CAP-2.12 Pattern of life and order of battle | full | partial | Entities with lineage and reports over journals; collection requirements and the tasking concurrence are built as of 2026-09-05 (GAP-005), with the lifecycle on the event bus; no designed pattern-of-life or order-of-battle product (GAP-019); a component is now named for every part of it by DN-11 and DN-19 (`../../design/`) | GAP-005, GAP-025 |
| CAP-3.1 Defended-asset list | full | none | Not in the `gungnir-config` schema; `gungnir-assessment` takes one protected point; a component is now named for every part of it by DN-01 (`../../design/`, first draft 2026-09-05) | GAP-026, GAP-073 |
| CAP-3.2 Threat scoring | full | partial | `gungnir-assessment` scoring and reward matrix; class lethality and asset weighting are extensions of the baseline | GAP-027, GAP-028 |
| CAP-3.3 Assignment recommendation | full | partial | `gungnir-allocation` value function and `gungnir-intercept-service`; effector layers and costs have no model in `ResourceConfig`; a component is now named for every part of it by DN-04 (`../../design/`, first draft 2026-09-05) | GAP-029, GAP-030 |
| CAP-3.4 Intercept geometry | full | none | `InterceptSolutionView` fields; solver is an `../../../ARCHITECTURE.md` §10 item | GAP-031 |
| CAP-3.5 Alternatives and rationale | full | partial | `gungnir-decision` rationale, alternatives, and what-if traits | GAP-032 |
| CAP-3.6 Rules of engagement | full | partial | `gungnir-policy` geofence and readiness chain; weapons control status, authority by role and class, escalation and timeout are not designed; a component is now named for every part of it by DN-08 and DN-09 (`../../design/`, first draft 2026-09-05) | GAP-028, GAP-033, GAP-034, GAP-072, GAP-052, GAP-088 |
| CAP-3.7 Queue under saturation | full | partial | `gungnir-command` pending list; ordering by priority and time remaining and pre-delegation are not designed; a component is now named for every part of it by DN-10 (`../../design/`, first draft 2026-09-05) | GAP-034, GAP-035 |
| CAP-3.8 Fires tasks | full | none | Plan and approval machinery are domain-neutral; no fires plan type, deconfliction rules, or handoff; a component is now named for every part of it by DN-05 (`../../design/`, first draft 2026-09-05) | GAP-036, GAP-090 |
| CAP-3.9 Sensor re-tasking | full | none | `gungnir-decision` is domain-neutral; no sensor-plan recommendation; a component is now named for every part of it by DN-13 (`../../design/`, first draft 2026-09-05) | GAP-037 |
| CAP-4.1 Present for decision | full | partial | Intercept panel in `gungnir-ui`; approval queue in `gungnir-workflow` layouts; plan 06 designs the panel | GAP-038, GAP-073 |
| CAP-4.2 Record every decision | full | partial | `InMemoryApprovalWorkflow` records every decision | GAP-028 |
| CAP-4.3 Never execute without a decision | full | full | Recommendation-only rule in `../../gungnir-capabilities.md` §5.4; `PolicyChain` never self-approves | GAP-039 |
| CAP-4.4 Handoff with provenance | full | none | `gungnir-api` v1 has submit and decide; no handoff endpoint in the ICD; a component is now named for every part of it by DN-07 (`../../design/`, first draft 2026-09-05) | GAP-040 |
| CAP-4.5 Warn assets and authorities | full | none | Alert lifecycle in `gungnir-workflow`; no warning function, threshold, or channel; a component is now named for every part of it by DN-03 (`../../design/`, first draft 2026-09-05) | GAP-042, GAP-090 |
| CAP-4.6 Track engagements and effects | full | none | `InterceptEvent` in the event schema; no engagement tracking or effect linkage; a component is now named for every part of it by DN-06 (`../../design/`, first draft 2026-09-05) | GAP-043 |
| CAP-4.7 Assist without authority | planned | none | Plan 08 (`../../plans/08-ai-agent-integration.md`) | GAP-044 |
| CAP-5.1 Journal | full | full | `gungnir-store` journal; fsync policy open in `../../performance-budgets.md` | GAP-085 |
| CAP-5.2 Replay and rehearse | partial | partial | `gungnir-replay` over journals; scenario replay through the live pipeline under a chosen plan is not designed; plan 07 for scenarios | GAP-071, GAP-045, GAP-046, GAP-076, GAP-051, GAP-087 |
| CAP-5.3 Reports and measures | full | partial | `gungnir-reporting` counts and export; measures per catalogue and a review workflow are not designed; a component is now named for every part of it by DN-20 (`../../design/`, first draft 2026-09-05) | GAP-071, GAP-079, GAP-047, GAP-048, GAP-049 |
| CAP-5.4 Disconnected and reconcile | full | partial | `gungnir-remote`, `gungnir-resilience`, `gungnir-collab`; mid-session failover waits on the transport | GAP-041, GAP-072, GAP-050 |
| CAP-5.5 Health and alert lifecycle | full | full | `gungnir-observability` and `gungnir-workflow` implemented | GAP-072 |
| CAP-5.6 Baselines and plans | full | partial | `ConfigBaseline` validation and store; asset list, policy configuration, and plan validity periods are not in the schema; a component is now named for every part of it by DN-01 and DN-08 (`../../design/`, first draft 2026-09-05) | GAP-071, GAP-051, GAP-052 |
| CAP-5.7 Model governance | full | partial | `gungnir-modelops` registry, wired 2026-09-05 (GAP-086, DN-24): the baseline declares mission profiles and candidate baselines, both binaries build the registry through the real promotion state machine and journal what the session opened with, and a rollback names the candidate it restored. Applying a promoted configuration to the picture waits on GAP-011 | GAP-077, GAP-078, GAP-079, GAP-081, GAP-083, GAP-086, GAP-053, GAP-067 |
| CAP-5.8 Battle rhythm | full | none | Reports exist; handover summaries and maintenance windows built 2026-09-05 (GAP-054, DN-21): the scheduler runs on mission time, planned downtime is distinguished from failure and from an overrun, and a handover is incomplete until acknowledged by name; scheduled-product delivery waits on GAP-040 and assistant drafts on GAP-044 | GAP-044, GAP-054 |
| CAP-5.9 Role workspaces and workflow | full | partial | `gungnir-workflow` role layouts, wired 2026-09-05 (GAP-055): the desktop draws the signed-in role's workspace, all eight layouts are transcribed from the UX documents and tested against them, and sixteen of the twenty panels are built. The four that are not name the entry that will build them, checked against the register by a test | GAP-068, GAP-070, GAP-074, GAP-075, GAP-087, GAP-089, GAP-055 |
| CAP-5.10 Performance budgets | full | none | `../../performance-budgets.md` budgets; harnesses named but absent | GAP-085, GAP-046, GAP-076, GAP-082, GAP-056 |
| CAP-6.1 Authenticate | full | none | Operator sessions are implemented for the disconnected desktop as of 2026-09-05 (GAP-057, DN-23 signed): local accounts verified with `argon2`, a back-off that never locks out, every attempt audited, and attribution that follows from a real sign-in. The node-issued token and the API caller check are not built; machine identity is GAP-060 | GAP-057 |
| CAP-6.2 Authorize by role, class, layer | partial | partial | `role_permits` coarse matrix; per-class and per-layer refinements are gaps (`roles-and-stakeholders.md` §4) | GAP-068, GAP-058 |
| CAP-6.3 Audit | full | partial | `gungnir-security` audit log; wiring pending | GAP-059 |
| CAP-6.4 Data protection | full | none | `../../../ARCHITECTURE.md` §8.5 states the intent; no component or key management designed; a component is now named for every part of it by DN-22 (`../../design/`, first draft 2026-09-05) | GAP-060, GAP-084 |
| CAP-6.5 Supply chain | full | partial | `deny.toml`, `release.yml`, `../../release-governance.md` | GAP-081, GAP-061, GAP-094 |
| CAP-6.6 Releasability | full | none | Open question in `../mission-analysis.md` §11; nothing designed; a component is now named for every part of it by DN-17 (`../../design/`, first draft 2026-09-05) | GAP-062 |
| CAP-6.7 Untrusted input | full | partial | Gateway treats input as data; plan 08 covers free text | GAP-044, GAP-076 |
| CAP-7.1 Versioned interface | full | partial | `../../gungnir-api-v1.md` contract; the read paths are served over HTTP and a WebSocket as of 2026-09-05 (GAP-041), loopback only and with the write paths refusing until a caller can be authenticated | GAP-041, GAP-063, GAP-066 |
| CAP-7.2 Interop standards | full | partial | `SchemaCatalog` with Arrow; codecs pending | GAP-069, GAP-063, GAP-064 |
| CAP-7.3 Three profiles | full | partial | `../../../ARCHITECTURE.md` §8 profiles; both binaries run | GAP-041 |
| CAP-7.4 Peer and coalition exchange | full | none | API and interop provide the mechanism; releasability and peer merge are not designed; a component is now named for every part of it by DN-18 (`../../design/`, first draft 2026-09-05) | GAP-065, GAP-091 |

## Counts

| Coverage | Design | Implementation |
|---|---|---|
| full | 53 | 4 |
| partial | 2 | 32 |
| none | 0 | 20 |
| planned | 1 | 0 |

Every capability whose design or implementation coverage is less than full has at
least one gap entry; the check is part of the generator that produced this draft
and must be repeated by hand when the register is edited.

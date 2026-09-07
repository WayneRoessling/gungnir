# Capability roadmap

Status: first draft, 2026-09-04. Maturity per capability per engineering increment
(none, partial, full), from `capability-statements.md`, and what each increment
delivers in mission terms. The increments are those of
`../../gungnir-capabilities.md` §7; the order of engineering work inside each is the
implementation order in the workspace `README.md`. This roadmap is the source for
the UAF strategic roadmap view (St-Rm, plan 03).

## What each increment delivers, in mission terms

| Increment | Mission outcome | Threads exercised end to end |
|---|---|---|
| I1 Productize the core | A desktop and a node that start, ingest recorded feeds through validation, show an honest picture with quality, journal everything, replay it, and report; decisions recorded once wired; nothing pretends to work | MT-09 (replay and reports), MT-10 (startup fallback) |
| I2 Integrate real data | Real and recorded sensors of the lead mission through live adapters; the tracking pipeline verified for Scenarios 1 to 3; cooperative identity; peer warnings from recorded feeds; coverage over terrain shown | MT-01 (single stream), MT-05, MT-06, MT-07 (partial) |
| I3 Close the decision loop | The allocator, intercept geometry, the defended-asset list, class lethality, the full rules-of-engagement model, the approval queue on screen, alternatives and what-if, warnings, the desktop assistant; the sector recommends, a human decides, and it is recorded | MT-01 to MT-04, MT-06, MT-08 |
| I4 Operationalize and scale | The API transport, connected profiles, mid-session failover and reconciliation, authentication for callers, releasability, industry codecs, learned models in shadow mode, performance budgets met, release governance exercised | MT-10 fully, MT-02 with peer warning, MT-05 and MT-08 with coalition exchange |

## Maturity by increment

| Capability | I1 | I2 | I3 | I4 |
|---|---|---|---|---|
| CAP-1.1 Ingest observations | partial | full | full | full |
| CAP-1.2 Validate and quarantine | full | full | full | full |
| CAP-1.3 Sensor modes and tasking | partial | full | full | full |
| CAP-1.4 Coverage and gaps | partial | partial | full | full |
| CAP-1.5 Time discipline | full | partial | full | full |
| CAP-1.6 Peer early warning | none | partial | partial | full |
| CAP-1.7 Cooperative identity | none | partial | full | full |
| CAP-2.1 Multi-sensor picture | partial | full | full | full |
| CAP-2.2 Tracks through gaps | partial | full | full | full |
| CAP-2.3 Sensor registration | none | full | full | full |
| CAP-2.4 Dense groups | none | partial | full | full |
| CAP-2.5 Clutter-tolerant surface picture | none | full | full | full |
| CAP-2.6 Classify and identify | partial | partial | full | full |
| CAP-2.7 Global identity | partial | partial | full | full |
| CAP-2.8 Predict trajectory and approach | partial | partial | full | full |
| CAP-2.9 Anomalies | none | partial | full | full |
| CAP-2.10 Terrain and map context | partial | partial | full | full |
| CAP-2.11 Geometric questions | full | full | full | full |
| CAP-2.12 Pattern of life and order of battle | none | partial | partial | full |
| CAP-3.1 Defended-asset list | none | partial | full | full |
| CAP-3.2 Threat scoring | partial | partial | full | full |
| CAP-3.3 Assignment recommendation | partial | partial | full | full |
| CAP-3.4 Intercept geometry | none | none | full | full |
| CAP-3.5 Alternatives and rationale | partial | partial | full | full |
| CAP-3.6 Rules of engagement | partial | partial | full | full |
| CAP-3.7 Queue under saturation | partial | partial | full | full |
| CAP-3.8 Fires tasks | none | none | full | full |
| CAP-3.9 Sensor re-tasking | none | partial | full | full |
| CAP-4.1 Present for decision | partial | partial | full | full |
| CAP-4.2 Record every decision | full | full | full | full |
| CAP-4.3 Never execute without a decision | full | full | full | full |
| CAP-4.4 Handoff with provenance | none | none | partial | full |
| CAP-4.5 Warn assets and authorities | none | partial | full | full |
| CAP-4.6 Track engagements and effects | none | partial | full | full |
| CAP-4.7 Assist without authority | none | none | partial | full |
| CAP-5.1 Journal | full | full | full | full |
| CAP-5.2 Replay and rehearse | partial | partial | full | full |
| CAP-5.3 Reports and measures | partial | partial | full | full |
| CAP-5.4 Disconnected and reconcile | partial | partial | partial | full |
| CAP-5.5 Health and alert lifecycle | full | full | full | full |
| CAP-5.6 Baselines and plans | full | full | full | full |
| CAP-5.7 Model governance | full | full | full | full |
| CAP-5.8 Battle rhythm | none | partial | partial | full |
| CAP-5.9 Role workspaces and workflow | partial | partial | full | full |
| CAP-5.10 Performance budgets | none | partial | partial | full |
| CAP-6.1 Authenticate | none | partial | full | full |
| CAP-6.2 Authorize by role, class, layer | full | full | full | full |
| CAP-6.3 Audit | partial | partial | full | full |
| CAP-6.4 Data protection | none | none | partial | full |
| CAP-6.5 Supply chain | full | full | full | full |
| CAP-6.6 Releasability | none | none | partial | full |
| CAP-6.7 Untrusted input | full | full | full | full |
| CAP-7.1 Versioned interface | partial | partial | partial | full |
| CAP-7.2 Interop standards | partial | partial | full | full |
| CAP-7.3 Three profiles | full | partial | partial | full |
| CAP-7.4 Peer and coalition exchange | none | partial | partial | full |

Note on I1: "full" in I1 means the capability's software is implemented and tested
as of 2026-09-04; the tracking mathematics that several partial capabilities depend
on is the largest single item of I2.

## Counts per increment

| Increment | full | partial | none |
|---|---|---|---|
| I1 | 13 | 22 | 21 |
| I2 | 17 | 33 | 6 |
| I3 | 44 | 12 | 0 |
| I4 | 56 | 0 | 0 |

## Dependencies that order the roadmap

- The tracking pipeline (CAP-2.1) gates CAP-2.2, CAP-2.4, CAP-2.5, CAP-2.8, and every
  Decide capability's move from partial to full.
- The allocator and intercept geometry (CAP-3.3, CAP-3.4) gate the policy checks on
  geometry (CAP-3.6) and handoff (CAP-4.4).
- The API transport gates CAP-1.6, CAP-4.4, CAP-5.4 (mid-session), CAP-6.1 (API
  callers), CAP-7.1, and CAP-7.4.
- The defended-asset list (CAP-3.1) gates CAP-3.2 and CAP-4.5 reaching full.
- The test-track suite (plan 07) gates CAP-5.2 rehearsal and CAP-5.10 measurement.

## Open items

- The owner confirms the maturity targets; plan 05 uses the I1 column as the
  baseline for the gap register and the I2 to I4 columns as target increments.

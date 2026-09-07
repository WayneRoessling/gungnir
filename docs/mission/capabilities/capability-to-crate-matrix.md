# Capability-to-crate matrix

Status: first draft, 2026-09-04. For each leaf capability, the crates and binaries
that provide it, with the crate's status from `../../gungnir-capabilities.md` §8
(T = trait surface, I = implemented and tested, W = implemented and wired into a
binary, P = plan not yet a crate). The last column is the capability's present
coverage from those crates, feeding plan 05: **none**, **partial**, **full**.

| Capability | Providing crates (status) | Coverage today |
|---|---|---|
| CAP-1.1 Ingest observations | `gungnir-ingest` (W: gateway, recorded, simulated), `gungnir-interop` (I: catalogue; T: codecs), `gungnir-model` (W) | partial |
| CAP-1.2 Validate and quarantine | `gungnir-ingest` (W: validation, quarantine, allow-list source authenticator), `gungnir-security` (T: authentication) | partial |
| CAP-1.3 Sensor modes and tasking | `gungnir-sensor-management` (I), `gungnir-config` (W) | partial |
| CAP-1.4 Coverage and gaps | `gungnir-analytics` (I), `gungnir-sensor-management` (I), `gungnir-viewport3d` (I: 2D fallback), `gungnir-data` (T) | partial |
| CAP-1.5 Time discipline | `gungnir-time` (W), `gungnir-fusion-async` (T) | partial |
| CAP-1.6 Peer early warning | `gungnir-api` (T), `gungnir-remote` (I: store-and-forward; transport pending), `gungnir-interop` (I) | none |
| CAP-1.7 Cooperative identity | `gungnir-ingest` (W), `gungnir-interop` (T: codecs), `gungnir-identification` (I) | none |
| CAP-2.1 Multi-sensor picture | `gungnir-tracking-service` (W: facade), `gungnir-fusion-async` (T), `gungnir-filters` (T), `gungnir-association` (T), `gungnir-track` (T), `gungnir-track-fusion` (T) | partial |
| CAP-2.2 Tracks through gaps | `gungnir-track` (T), `gungnir-model` (W), `gungnir-ui` (W), `gungnir-assessment` (I) | partial |
| CAP-2.3 Sensor registration | `gungnir-track-fusion` (T), `gungnir-data-fusion` (T) | none |
| CAP-2.4 Dense groups | `gungnir-rfs` (T) | none |
| CAP-2.5 Clutter-tolerant surface picture | `gungnir-association` (T), `gungnir-track` (T), `gungnir-scenario` (T) | none |
| CAP-2.6 Classify and identify | `gungnir-identification` (I), `gungnir-policy` (I), `gungnir-ml` (P, plan 09) | partial |
| CAP-2.7 Global identity | `gungnir-identity` (I) | partial |
| CAP-2.8 Predict trajectory and approach | `gungnir-assessment` (I), `gungnir-filters` (T) | partial |
| CAP-2.9 Anomalies | `gungnir-observability` (I), `gungnir-ml` (P) | none |
| CAP-2.10 Terrain and map context | `gungnir-data` (T), `gungnir-data-fusion` (T), `gungnir-geo` (I), `gungnir-viewport3d` (I: 2D fallback) | partial |
| CAP-2.11 Geometric questions | `gungnir-analytics` (I) | full |
| CAP-2.12 Pattern of life and order of battle | `gungnir-identity` (I), `gungnir-replay` (I), `gungnir-reporting` (I) | partial |
| CAP-3.1 Defended-asset list | `gungnir-config` (W), `gungnir-assessment` (I) | none |
| CAP-3.2 Threat scoring | `gungnir-assessment` (I) | partial |
| CAP-3.3 Assignment recommendation | `gungnir-intercept-service` (W: facade), `gungnir-allocation` (T: `NotImplemented`), `gungnir-assessment` (I) | partial |
| CAP-3.4 Intercept geometry | `gungnir-intercept-service` (W: fields), `gungnir-coord` (T) | none |
| CAP-3.5 Alternatives and rationale | `gungnir-decision` (I: rationale; T: alternatives, what-if) | partial |
| CAP-3.6 Rules of engagement | `gungnir-policy` (I), `gungnir-geo` (I), `gungnir-security` (I) | partial |
| CAP-3.7 Queue under saturation | `gungnir-command` (I), `gungnir-workflow` (I) | partial |
| CAP-3.8 Fires tasks | `gungnir-intercept-service`, `gungnir-policy`, `gungnir-command` (domain-neutral) | none |
| CAP-3.9 Sensor re-tasking | `gungnir-sensor-management` (I), `gungnir-analytics` (I), `gungnir-decision` (T) | none |
| CAP-4.1 Present for decision | `gungnir-ui` (W: intercept panel), `gungnir-workflow` (I) | partial |
| CAP-4.2 Record every decision | `gungnir-command` (I), `gungnir-security` (I: audit) | partial |
| CAP-4.3 Never execute without a decision | `gungnir-policy` (I), `gungnir-command` (I); design rule in every crate | full (by design; enforced by review and test) |
| CAP-4.4 Handoff with provenance | `gungnir-api` (T), `gungnir-remote` (I) | none |
| CAP-4.5 Warn assets and authorities | `gungnir-workflow` (I), `gungnir-observability` (I), `gungnir-api` (T) | none |
| CAP-4.6 Track engagements and effects | `gungnir-model` (W: events), `gungnir-intercept-service` (W) | none |
| CAP-4.7 Assist without authority | `gungnir-agent` (P, plan 08) | none |
| CAP-5.1 Journal | `gungnir-store` (W) | full |
| CAP-5.2 Replay and rehearse | `gungnir-replay` (I), `gungnir-mission` (T), test tracks (P, plan 07) | partial |
| CAP-5.3 Reports and measures | `gungnir-reporting` (I), `gungnir-metrics` (T) | partial |
| CAP-5.4 Disconnected and reconcile | `gungnir-remote` (I), `gungnir-resilience` (I), `gungnir-collab` (I), `gungnir-app` (W: startup fallback) | partial |
| CAP-5.5 Health and alert lifecycle | `gungnir-observability` (W in node), `gungnir-workflow` (I) | full |
| CAP-5.6 Baselines and plans | `gungnir-config` (W), `gungnir-mission` (T) | partial |
| CAP-5.7 Model governance | `gungnir-modelops` (I) | partial |
| CAP-5.8 Battle rhythm | `gungnir-reporting` (I), `gungnir-agent` (P) | none |
| CAP-5.9 Role workspaces and workflow | `gungnir-workflow` (I), `gungnir-ui` (W) | partial |
| CAP-5.10 Performance budgets | all crates; benches (placeholders) | none |
| CAP-6.1 Authenticate | `gungnir-security` (T) | none |
| CAP-6.2 Authorize by role, class, layer | `gungnir-security` (I: coarse), `gungnir-policy` (I) | partial |
| CAP-6.3 Audit | `gungnir-security` (I: audit log) | partial |
| CAP-6.4 Data protection | `gungnir-api` (T), `gungnir-store` (W), deployment | none |
| CAP-6.5 Supply chain | `deny.toml`, `release.yml` | partial (workflow present; unexercised) |
| CAP-6.6 Releasability | `gungnir-security` (I), `gungnir-api` (T) | none |
| CAP-6.7 Untrusted input | `gungnir-ingest` (W), `gungnir-agent` (P) | partial |
| CAP-7.1 Versioned interface | `gungnir-api` (T: types, ICD) | partial |
| CAP-7.2 Interop standards | `gungnir-interop` (I: catalogue, Arrow; T: codecs) | partial |
| CAP-7.3 Three profiles | `gungnir-app` (W), `gungnir-node` (W), `gungnir-remote` (I), `gungnir-config` (W) | partial |
| CAP-7.4 Peer and coalition exchange | `gungnir-api` (T), `gungnir-interop` (I), `gungnir-security` (I) | none |

## Coverage summary

| Coverage | Count | Capabilities |
|---|---|---|
| full | 4 | CAP-2.11, CAP-4.3, CAP-5.1, CAP-5.5 |
| partial | 32 | the rest not listed as none |
| none | 20 | CAP-1.6, CAP-1.7, CAP-2.3, CAP-2.4, CAP-2.5, CAP-2.9, CAP-3.1, CAP-3.4, CAP-3.8, CAP-3.9, CAP-4.4, CAP-4.5, CAP-4.6, CAP-4.7, CAP-5.8, CAP-5.10, CAP-6.1, CAP-6.4, CAP-6.6, CAP-7.4 |

The twenty with no coverage are the first input to plan 05's gap register; the
partial ones each name, in `capability-statements.md`, what is missing.

# Capability-to-thread matrix

Status: first draft, 2026-09-04. An `x` means the thread cannot complete to its
success condition without the capability; `s` means the capability supports the
thread but a workaround exists. Threads are in `../mission-threads.md`.

| Capability | MT-01 | MT-02 | MT-03 | MT-04 | MT-05 | MT-06 | MT-07 | MT-08 | MT-09 | MT-10 |
|---|---|---|---|---|---|---|---|---|---|---|
| CAP-1.1 Ingest observations | x | x | x | x | x | x | x | x | | s |
| CAP-1.2 Validate and quarantine | x | x | x | x | x | x | x | x | | x |
| CAP-1.3 Sensor modes and tasking | s | s | x | s | s | s | x | x | x | |
| CAP-1.4 Coverage and gaps | s | s | | | | | x | | x | |
| CAP-1.5 Time discipline | x | x | | x | | x | x | | x | x |
| CAP-1.6 Peer early warning | x | x | | x | x | | | x | | |
| CAP-1.7 Cooperative identity | s | x | s | x | x | | | x | | |
| CAP-2.1 Multi-sensor picture | x | x | x | x | x | x | x | x | | |
| CAP-2.2 Tracks through gaps | x | x | | x | | x | x | | | |
| CAP-2.3 Sensor registration | s | | | | | x | | | | |
| CAP-2.4 Dense groups | x | x | | | | | | | | |
| CAP-2.5 Clutter-tolerant surface picture | | | | x | x | | | | | |
| CAP-2.6 Classify and identify | x | x | x | x | x | x | | x | | |
| CAP-2.7 Global identity | | | | | x | x | | x | | |
| CAP-2.8 Predict trajectory and approach | x | x | s | x | | x | | | | |
| CAP-2.9 Anomalies | | | | | x | x | | | | |
| CAP-2.10 Terrain and map context | s | s | s | s | s | s | s | | x | |
| CAP-2.11 Geometric questions | | | | | | x | x | | x | |
| CAP-2.12 Pattern of life and order of battle | | | | | | s | | x | x | |
| CAP-3.1 Defended-asset list | x | x | | x | | | | | x | |
| CAP-3.2 Threat scoring | x | x | s | x | | x | | | | |
| CAP-3.3 Assignment recommendation | x | x | s | x | | x | | | | |
| CAP-3.4 Intercept geometry | x | x | | x | | | | | | |
| CAP-3.5 Alternatives and rationale | x | x | | | | x | | | x | |
| CAP-3.6 Rules of engagement | x | x | x | x | | x | | | x | x |
| CAP-3.7 Queue under saturation | x | x | | | | | | | | |
| CAP-3.8 Fires tasks | | | | | | x | | | | |
| CAP-3.9 Sensor re-tasking | | | | | | | x | | s | |
| CAP-4.1 Present for decision | x | x | x | x | | x | | | | |
| CAP-4.2 Record every decision | x | x | x | x | | x | | | | x |
| CAP-4.3 Never execute without a decision | x | x | x | x | | x | | | | x |
| CAP-4.4 Handoff with provenance | x | x | s | x | | x | | | | |
| CAP-4.5 Warn assets and authorities | x | x | | x | | | | | | |
| CAP-4.6 Track engagements and effects | x | x | s | x | | x | | | | |
| CAP-4.7 Assist without authority | s | | | | | | s | s | s | |
| CAP-5.1 Journal | x | x | x | x | x | x | x | x | x | x |
| CAP-5.2 Replay and rehearse | | | | | | | | | x | |
| CAP-5.3 Reports and measures | | | | | | | | | x | |
| CAP-5.4 Disconnected and reconcile | | | | | | | | | | x |
| CAP-5.5 Health and alert lifecycle | x | x | x | x | s | s | x | | | x |
| CAP-5.6 Baselines and plans | | | | | | | | | x | s |
| CAP-5.7 Model governance | | | | | | | | | x | |
| CAP-5.8 Battle rhythm | | | | | | | | | x | |
| CAP-5.9 Role workspaces and workflow | x | x | x | x | x | x | x | x | x | x |
| CAP-5.10 Performance budgets | x | x | x | x | x | x | s | | | s |
| CAP-6.1 Authenticate | x | x | x | x | x | x | x | x | x | x |
| CAP-6.2 Authorize by role, class, layer | x | x | x | x | | x | s | s | x | x |
| CAP-6.3 Audit | x | x | x | x | | x | x | x | x | x |
| CAP-6.4 Data protection | | | | | | | | s | | x |
| CAP-6.5 Supply chain | | | | | | | | | x | |
| CAP-6.6 Releasability | | | | | x | | | x | | |
| CAP-6.7 Untrusted input | x | x | x | x | x | x | x | x | | x |
| CAP-7.1 Versioned interface | x | x | | x | x | | | x | | x |
| CAP-7.2 Interop standards | s | | | s | x | s | | x | | |
| CAP-7.3 Three profiles | | | | | | | | | | x |
| CAP-7.4 Peer and coalition exchange | x | x | | s | x | | | x | | |

## Coverage check

Every capability serves at least one thread and every thread uses at least one
capability from Sense, Sustain, and Secure. Capabilities used by only one thread
(CAP-3.8, CAP-5.2, CAP-5.3, CAP-5.4, CAP-5.7, CAP-5.8, CAP-6.5, CAP-7.3) are not
therefore less important; they are the capabilities that give the fires task and
the sustainment threads MT-09 and MT-10 their distinct value.

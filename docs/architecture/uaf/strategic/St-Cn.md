# St-Cn Capability connectivity

**UAF definition.** The strategic connectivity view shows the dependencies between
capabilities: which capability needs another to deliver its outcome.

**Purpose here.** The dependency chains that order the roadmap: what must exist
before a capability can be full. It is the same dependency list the closure roadmap
uses (`../../../mission/gap-analysis/closure-roadmap.md`, "Dependencies that order
the roadmap"), drawn as a graph.

Status: first draft, 2026-09-04.

Diagram: [`St-Cn.puml`](St-Cn.puml).

## Dependencies

| Capability | Depends on | Why |
|---|---|---|
| CAP-2.2, CAP-2.4, CAP-2.5, CAP-2.8 | CAP-2.1 | Coasting, dense groups, clutter tolerance, and prediction are behaviours of the pipeline |
| CAP-2.3 | CAP-2.1, CAP-2.10 | Registration corrects the fused picture; point-cloud registration supplies evidence |
| CAP-2.6 | CAP-1.7, CAP-2.1 | Evidence comes from cooperative sources and the picture |
| CAP-2.7, CAP-2.12 | CAP-2.6, CAP-5.1 | Identity across sessions needs class evidence and the journal |
| CAP-3.2 | CAP-3.1, CAP-2.8 | Scores are relative to the asset list and time to impact |
| CAP-3.3 | CAP-3.2, CAP-2.1 | The allocator needs scores and a real picture |
| CAP-3.4 | CAP-3.3 | Geometry is computed for assignments |
| CAP-3.6 | CAP-3.4, CAP-6.2 | Geofence checks need a point; authority checks need the role matrix |
| CAP-3.5, CAP-3.7 | CAP-3.3, CAP-3.6 | Alternatives and the queue are built on policy-checked plans |
| CAP-3.8 | CAP-3.3, CAP-3.6 | A fires task is a plan under deconfliction rules |
| CAP-3.9 | CAP-1.3, CAP-1.4 | Re-tasking recommendations need modes and coverage |
| CAP-4.1, CAP-4.2 | CAP-3.6 | Only policy-checked plans are presented and decided |
| CAP-4.4, CAP-4.5, CAP-4.6 | CAP-4.2, CAP-7.1 | Handoff, warning, and effects follow a decision over the interface |
| CAP-4.7 | CAP-6.7, CAP-6.3 | The assistant is bounded by untrusted-input handling and audit |
| CAP-5.2, CAP-5.3 | CAP-5.1 | Replay and reports read the journal |
| CAP-5.4 | CAP-5.1, CAP-7.1, CAP-7.3 | Reconciliation merges journals over the interface across profiles |
| CAP-5.7 | CAP-5.6 | Baselines are configuration |
| CAP-6.2 | CAP-6.1 | Authorization needs an authenticated identity |
| CAP-6.4 | CAP-6.1, CAP-7.1 | Transit protection rides the transport with machine credentials |
| CAP-6.6 | CAP-7.4 | Marking exists to govern exchange |
| CAP-1.6, CAP-7.4 | CAP-7.1, CAP-7.2 | Peer exchange needs the interface and the formats |

## Elements used

- The leaf capabilities named above.

## Notes

- Dependencies are stated at capability level; the engineering dependencies between
  gaps are in the closure roadmap and were checked for cycles there.
- CAP-4.3 depends on nothing and everything depends on it: it is the design rule,
  not a node in the graph.

## Traceability

- Derives from: St-Tx; `../../../mission/gap-analysis/closure-roadmap.md`;
  `../../../mission/capabilities/capability-statements.md` (the "Provided by" lines).
- Feeds: St-Rm (the order of increments), Pj-Rm.

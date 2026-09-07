# Dependency edges

Status: **reviewed and accepted by the engineering reviewer, 2026-09-05; edges (h), (i) and (j) accepted 2026-09-06 (§7).** Every dependency
edge the design set adds, in one place, so the graph is reviewed once rather than
twenty-two times.

All five edges are accepted, including the one flagged in §4 as the weakest. Each is
cleared to enter a manifest, and is drawn in `ARCHITECTURE.md` §7.1 in the change that adds
it, never before.

**The rule, decided by the owner on 2026-09-05:** new edges are allowed where the coupling
is natural, each drawn in `ARCHITECTURE.md` §7.1 in the same change that introduces it.

**Added after the review, and not covered by it (2026-09-05, GAP-086):** `gungnir-app` and
`gungnir-node` to `gungnir-modelops`, both downward from a binary, which §7.1 already
describes as depending on everything above it; and **`gungnir-modelops` to `gungnir-model`,
which DN-24 §5 did not list** -- the crate was one of the four the graph names as not using
the model, and identity has to live there because `Provenance` does. All three are in the
manifests and drawn in §7.1 as edge (h), and the owner signed the third as a correction to
DN-24 §5 on 2026-09-05. **None was seen by the engineering reviewer**, which is a separate
review from the owner's sign-off and is still outstanding for these three.
Every edge is argued in its note, checked for cycles, and may be rejected by the
engineering reviewer in favour of passing the data in.

## 1. Edges taken

Five, all verified acyclic against the current graph with all five applied together on
2026-09-05.

| Edge | Note | What it buys | What it would otherwise duplicate |
|---|---|---|---|
| `gungnir-workflow` to `gungnir-assessment` | DN-03 | The warning rule reads `AssetExposure` and `ClosestApproach` to know when an obligation triggers | The trigger rule would live in both binaries, untested and with no second consumer, breaking AP-13 |
| `gungnir-workflow` to `gungnir-sensor-management` | DN-11 | The tasking case shows the state of the sensor tasks serving a requirement | The join between a requirement and its tasks, in both binaries |
| `gungnir-analytics` to `gungnir-sensor-management` | DN-12 | `coverage_from_registry` builds coverage input from live sensor records | The `SensorRecord` to `CoverageVolume` mapping, in the app, the node, and the recommender |
| `gungnir-decision` to `gungnir-analytics` | DN-13 | The sensor-plan search evaluates coverage per candidate, inside its own loop | Nothing. A search that cannot evaluate its own candidates is not a search |
| `gungnir-reporting` to `gungnir-identity` | DN-19 | An order of battle is a list of entities with lineage, not of tracks | The lineage, which is what makes an entry defensible |
| `gungnir-ingest` to `gungnir-interop` | GAP-001 (no note; `ARCHITECTURE.md` §8.6 described it) | The ASTERIX radar adapter decodes with the Category 048 and 034 codecs | A second ASTERIX parser inside the adapter, which is the outcome GAP-064 exists to prevent |

## 2. Depth after the change

Longest dependency path from each affected crate, which is the number that says whether the
graph is getting deeper rather than merely wider:

| Crate | Depth after |
|---|---|
| `gungnir-assessment` | 3 |
| `gungnir-identity` | 3 |
| `gungnir-sensor-management` | 4 |
| `gungnir-analytics` | 5 |
| `gungnir-workflow` | 5 |
| `gungnir-decision` | 6 |
| `gungnir-reporting` | 6 |

Nothing exceeds the depth the productization layer already had. The graph gets wider, not
deeper.

## 3. Edges deliberately refused

Four, and each refusal is part of a design rather than an omission.

| Edge not taken | Note | Why |
|---|---|---|
| `gungnir-assessment` to `gungnir-config` | DN-01 | The asset type goes in `gungnir-model` instead, under AP-06. Configuration stays out of the scoring path, and a replayed session carries its own asset list without reading a baseline file. The plan's edge table flagged this one as allowed-but-not-recommended, and the recommendation stands |
| `gungnir-intercept-service` to `gungnir-command` | DN-06 | **Impossible under AP-10.** A service facade may not depend on a productization crate. Engagement state keys on `DecisionId`, a new model identifier, which is the better design anyway: the facade should not know how approvals are stored |
| `gungnir-policy` to `gungnir-security` | DN-09 | Authority rules name roles as strings, validated against the adopted set when the baseline is loaded. That puts the spelling check where a misspelling can be reported to a person, and keeps the policy engine free of a security dependency |
| `gungnir-analytics` to anything, for the detectors | DN-15 | D-13 decided on 2026-09-04 that the anomaly detectors are pure functions with no new edge. A resolved decision stands; the 2026-09-05 rule applies to closures decided from that date |

## 4. The one that was argued about

`gungnir-analytics` to `gungnir-sensor-management` (DN-12) was the weakest of the five and
was flagged for the engineering reviewer, who **accepted it on 2026-09-05**. The reasoning
that led to the flag is kept below, because it is the argument to revisit if analytics
later grows further.

Today `gungnir-analytics` depends on `gungnir-coord`, `gungnir-data`, and `gungnir-geo`,
all geometry. Adding `gungnir-sensor-management` makes it depend on a
configuration-reading crate and moves it from a geometry library toward a central one. It
is legitimate and acyclic, and the reviewer may still prefer the conversion to live in the
binaries.

It was not rejected. Had it been, only `coverage_from_registry` would have moved and
nothing else in DN-12 or DN-13 would have changed, because the pure function
`combined_coverage` is defined to take sensor volumes rather than a registry. That property
is worth keeping: it is what makes the edge reversible.

`gungnir-workflow` to `gungnir-sensor-management` (DN-11) is the second weakest and is
narrow by construction: workflow reads task state and never issues a command, so exactly
one crate can talk to a sensor.

## 4a. Which edges are in a manifest

| Edge | Note | In a manifest | Drawn in §7.1 |
|---|---|---|---|
| `gungnir-analytics` to `gungnir-sensor-management` | DN-12 | 2026-09-05 | Yes, as (c) |
| `gungnir-decision` to `gungnir-analytics` | DN-13 | 2026-09-05 | Yes, as (a) |
| `gungnir-workflow` to `gungnir-sensor-management` | DN-11 | 2026-09-05 | Yes, as (b) |
| `gungnir-workflow` to `gungnir-assessment` | DN-03 | 2026-09-05 | Yes, as (e) |
| `gungnir-reporting` to `gungnir-identity` | DN-19 | 2026-09-05 | Yes, as (d) |
| `gungnir-ingest` to `gungnir-interop` | GAP-001 | 2026-09-06 | Yes, as (j) |
| `gungnir-app` to `gungnir-identification` | GAP-010 | 2026-09-06 | Yes, as (n) |
| `gungnir-app` to `gungnir-identity` | GAP-019, GAP-025 | 2026-09-06 | Yes, as (o) |
| `gungnir-remote` to `gungnir-interop` | DN-25, GAP-091 | **No: accepted 2026-09-06, no code yet** | Not yet; §7.1 gains (s) in the change that adds the sink |
| `gungnir-remote` to `gungnir-security` | GAP-060, D-29 | 2026-09-06, with the `identity.rs` move out of `gungnir-node` | Yes, as (t) -- **the entry is dated 2026-09-07; the manifest line is older, see §14** |

**All five are in manifests and drawn.** The acyclicity check was re-run with the full set
on 2026-09-05: 154 crate-to-crate edges, no cycle.

DN-12 brought a fourth edge the design did not anticipate: `gungnir-analytics` to
`gungnir-model`, because the coverage types name sensors by `SensorId` and the model owns
it. It is the same direction and the same layer as the accepted one, and analytics is no
longer independent of the model. That is recorded here rather than absorbed, because the
graph's note that analytics stood outside the model was true and is not any more.

One move avoided a sixth edge rather than adding one. `SessionId` lived in
`gungnir-store`, and the review case in `gungnir-workflow` needed it. Six crates share the
type, so it moved down to `gungnir-model` with the store re-exporting it, which is
`agentic-coding-standards.md` §1.2 rather than a new dependency. DN-20 had assumed it was
already a model type.

## 5. What lands in `ARCHITECTURE.md`

Each edge is drawn in §7.1's graph in the change that adds it to a manifest, never before
and never after. The compliance assessment's dependency-direction check walks the manifests
against that drawing, and GAP-081 automates it, so an edge added without the drawing fails
the build once that gap closes.

**Corrected 2026-09-06**: the script was discarded after the 2026-09-04 run and was not
re-run by hand at any boundary. The direction check is now
`gungnir-app/tests/dependency_graph.rs` (§6), which runs with every `cargo test`.

## 6. Acyclicity check

Reproduced here so it can be re-run rather than trusted:

1. Read every `gungnir-*/Cargo.toml` and collect the `gungnir-*` path dependencies.
2. Add the five edges above.
3. Depth-first search with three-colour marking; a back edge is a cycle.

Result on 2026-09-05: **no cycle, with all five applied.**

**Corrected 2026-09-06.** This section said the check was part of GAP-081's
continuous-integration job. It was not: the check was a script written for the
2026-09-04 assessment and discarded, GAP-081 is open, and every edge review since --
including the one recorded above -- rested on prose. The check now exists as
`gungnir-app/tests/dependency_graph.rs`, which reads every `gungnir-*/Cargo.toml` and
fails on a cycle, an upward edge by layer (§1.1, AP-10), a crate not placed in the layer
table, or a recorded edge missing from the manifests. It runs with `cargo test`. It is
the first of GAP-081's five checks and is recorded against that entry.

## 7. Edges (h), (i) and (j) -- **accepted by the owner as engineering reviewer, 2026-09-06**

Five edges were added after the 2026-09-05 review and none was seen by it: (h)
`gungnir-app` and `gungnir-node` to `gungnir-modelops`, and `gungnir-modelops` to
`gungnir-model` (GAP-086, DN-24); (i) `gungnir-app` to `gungnir-decision` (GAP-037,
DN-13); (j) `gungnir-ingest` to `gungnir-interop` (GAP-001, the ASTERIX radar adapter,
landed by a parallel session the same day). This is the record drafted for one
acceptance covering all five, drafted and accepted the same day. What the reviewer
accepted, and the evidence for each:

| Edge | What it is | Evidence |
|---|---|---|
| app → modelops, node → modelops, app → decision | A binary reaching a productization crate. The graph in `ARCHITECTURE.md` already draws `gungnir-app ──► everything above it`, and (g) established node → productization on 2026-09-05. These were drawn because each crate had **no dependents at all** before them, which is a finding about the crates, not a new class of edge | `dependency_graph.rs`: acyclic; `Binary → Productization` is downward; both crates placed in the productization layer |
| modelops → model | A productization crate joining the model. The identity type `AlgorithmBaselineId` has to live in the model because `Provenance` carries one; the alternative was two bare strings assembled in every caller, a second answer to "what is a baseline" outside the crate that owns them (DN-24 §5 correction, signed) | `dependency_graph.rs`: `Productization → Model` is downward; no cycle |
| (j) ingest → interop | A productization crate reaching a sibling. `ARCHITECTURE.md` §8.6 described this edge from the first draft ("`gungnir-ingest` adapters using `gungnir-interop` codecs"); no manifest carried it because no adapter existed. `gungnir-interop` depends on `gungnir-model` alone, so the edge adds no transitive reach | `dependency_graph.rs`: `Productization → Productization` is permitted and acyclic; the test's recorded-edge list names (j) |
| (i) specifically | DN-13 §4 designed the split: "the caller enumerates candidates because only the registry knows which mode transitions are legal, and this crate may not depend on it." The app edge is the one the note intended; the refused alternative was decision → sensor-management | `sustainment::sensor_plans` builds candidates from the registry's transition table and hands them to the planner; `gungnir-decision`'s manifest has no sensor-management edge |

The reviewer-agent checklist (`../agentic-workflow.md`, review pipeline), walked
against the two changes that added the edges:

- *No edge the graph does not already imply*: none. Three are licensed by the binary
  line of the graph; the fourth is to the bottom layer. The test above now enforces the
  direction rule mechanically, which it did not before.
- *No crate added to the stack without §2.9*: none added.
- *For `gungnir-app` and `gungnir-node`, wiring not logic*: `sustainment::sensor_plans`
  enumerates candidates and converts frames; the scoring is `SensorPlanner::recommend`.
  `governance.rs` builds the registry from the baseline and journals; the gate is
  `InMemoryModelRegistry::from_baseline`. Both are wiring.
- *No health flag, connection state, or test claims a subsystem works when it does not*:
  `NoSensorPlan` keeps "no change helps" apart from "could not evaluate";
  `GovernedProfiles::Refused` and `NoneInForce` say when nothing is in force.
- *Numerical stability, `unwrap()`, doc citations*: no numerics added; no `unwrap()`
  outside tests; every citation named a section that exists at the time of writing.

Accepted by the owner on 2026-09-06, as engineering reviewer, on the evidence above and
with `gungnir-app/tests/dependency_graph.rs` passing; the same line stands in
`ARCHITECTURE.md` §7.1 against (h), (i) and (j). Every edge in the graph is now reviewed.

## 7a. Edges (k) and (l) -- **accepted by the owner as engineering reviewer, 2026-09-06**

Added by GAP-028's node half, after the §7 acceptance, and accepted the same day on the evidence below with `gungnir-app/tests/dependency_graph.rs` passing.

| Edge | What it is | Evidence |
|---|---|---|
| (k) node → policy | The binary reaching the productization crate whose chain the desktop already runs; the node evaluates geofence and control status on every fresh plan and publishes the verdict with the engine list, and deliberately neither queues (DN-23 §4, GAP-057) nor evaluates authority (no asking role) | `dependency_graph.rs`: `Binary → Productization`, acyclic, listed |
| (l) node → geo | Anticipated by `ARCHITECTURE.md` §8's profile table in its first draft ("a node needs `gungnir-geo` only if geofence policy is evaluated server-side"). The service it supplies is empty until GAP-088 gives geofences a configuration source, and the node says so at start | `dependency_graph.rs`: same; the node's start-up warning |

## 9. Edge (m) -- `gungnir-app` to `gungnir-resilience` (2026-09-06, GAP-050)

Added by the reconciliation half of GAP-050 and recorded here in the same change, with
`gungnir-app/tests/dependency_graph.rs` naming it.

| Edge | What it is | Evidence |
|---|---|---|
| (m) app → resilience | The binary reaching a productization crate. `ARCHITECTURE.md` §7.1 draws `gungnir-app ──► everything above it`, and `gungnir-resilience` (model, eventing, store) had **no dependent at all** before this: `reconcile` existed and nothing ran it. The desktop runs it over its own journal and the node's `GET /v2/history` after an outage, and PN-18 shows the report. The refused alternative was computing the merge on the node, which would have put the desktop's journal on the wire before a person had seen the conflicts | `dependency_graph.rs`: `Binary → Productization` is downward, acyclic, listed as (m); `gungnir-app/tests/failover.rs` |

## 10. Edges (n) and (o) -- **accepted by the owner as engineering reviewer, 2026-09-06**

`gungnir-app` to `gungnir-identification` and to `gungnir-identity`. Added by GAP-010
(the first evidence source) and GAP-019 with GAP-025 (the resolver and the order of
battle), recorded here in the same change, with `gungnir-app/tests/dependency_graph.rs`
naming both. Reviewed and accepted the same day.

| Edge | What it is | Evidence |
|---|---|---|
| (n) app → identification | The binary reaching the productization crate that fuses evidence. `EvidenceFusionEngine` had been built with settings (GAP-018) and nothing constructed it, because nothing produced evidence; the AIS adapter does now, and the desktop is where the tracks it is associated with live. The refused alternative was fusing on the node, which has no tracks until GAP-011. **That reason expired 2026-09-06**: GAP-011 is closed and the node has tracks, so the edge stands on the second half of its argument -- the desktop is where the tracks are -- and a node-side alternative would now be a fresh decision rather than an impossibility. Recorded here rather than left to be read as still-current |  `dependency_graph.rs`: `Binary → Productization`, acyclic, listed as (n); `gungnir-app/tests/cooperative_identity.rs` |
| (o) app → identity | The binary reaching the resolver. The desktop holds several sessions of its own journal and is the host that can fold them; `gungnir-reporting` already had its edge to `gungnir-identity` (d), so the app reaching both is the same direction. The refused alternative was a resolver inside `gungnir-reporting`, which would make a report own identity | `dependency_graph.rs`: listed as (o); `gungnir-app/tests/order_of_battle.rs` |

## 11. Edges (p) and (q) -- **accepted by the owner as engineering reviewer, 2026-09-06**

`gungnir-node` to `gungnir-remote`; `gungnir-ml` to `gungnir-model` and
`gungnir-interop`. Recorded in the same change as the manifests, with
`gungnir-app/tests/dependency_graph.rs` naming both. Reviewed and accepted the same day.

| Edge | What it is | Evidence |
|---|---|---|
| (p) node → remote | The node binding a peer link (GAP-009). A peer link is a client link to a partner's node and the client lives in `gungnir-remote`; the refused alternative was a second client inside the node, which is the duplication the crate exists to prevent. Downward from the binary into productization; acyclic | `dependency_graph.rs`; `gungnir-remote/tests/tls_link.rs` |
| (q) ml → model, interop | The crate `docs/ml/architecture.md` §1 drew, now existing (GAP-077, GAP-079): the traits and the dataset extraction over model views, and the dataset schema as a catalogue entry so a dataset and the wire share one definition. Nothing depends on it; the consumers take model output through their existing traits when GAP-080 promotes one | `dependency_graph.rs`; `gungnir-ml/tests/dataset.rs` |

One dev-only edge was added and is not drawn, per the convention for `gungnir-scenario`:
`gungnir-app` dev-depends on `gungnir-api` for the end-to-end failover test, which runs a
real node transport in the test process (GAP-050). The runtime edge stays through
`gungnir-remote`.

## 13. Edge (s) -- **accepted by the owner as engineering reviewer, 2026-09-06**

`gungnir-node` to `gungnir-identity`. Added by GAP-019, recorded here in the same change,
with `gungnir-app/tests/dependency_graph.rs` naming it. Reviewed and accepted the same day.

| Edge | What it is | Evidence |
|---|---|---|
| (s) node → identity | The node resolving cross-session entity identity **for the record**. `gungnir-store`'s journal on a node is the authoritative account of a mission (§8.1), and a deployment whose desktops were not connected for part of a watch has nowhere else the correlation could have been made. The desktop's resolver answers the same question for a panel to draw; this one answers it for the account. The refused alternative was leaving it to the desktop alone, which loses the correlation for any period no desktop was attached -- the period a journal most needs to cover. `gungnir-identity` is productization and depends on `gungnir-model` alone; downward from a binary; acyclic | `dependency_graph.rs`: `Binary → Productization`, acyclic, listed as (s); `gungnir-node/src/entities.rs` and its tests |

**A second edge was considered and not taken.** A node-side order of battle or pattern of
life would need `gungnir-node` to `gungnir-reporting` as well. It was refused: the node has
no panel to draw a product on, and an edge carrying products nothing displays would be an
edge added for tidiness rather than for a caller -- the unwired pattern this register keeps
finding. The desktop assembles those over the same lineages from its own journal fold.

**And a note on edge (n).** Its recorded justification was that the node "has no tracks
until GAP-011", which expired on 2026-09-06 when GAP-011 closed. Edge (s) does not
contradict it: identity correlation belongs where the journal is, and evidence fusion
belongs where the operator working the picture is. Edge (n) now stands on that second half
of its argument rather than on an impossibility.

## 12. Edge (r) -- **accepted by the owner as engineering reviewer, 2026-09-06**

`gungnir-fusion-async` to `gungnir-core`, `gungnir-filters` and `gungnir-association`.
Added by the tracking pipeline (GAP-011), recorded here in the same change, with
`gungnir-app/tests/dependency_graph.rs` naming it. Reviewed and accepted the same day.

| Edge | What it is | Evidence |
|---|---|---|
| (r) fusion-async → core, filters, association | The pipeline predicts a track to a measurement's time, gates the measurement, assigns and updates: a motion model, a filter and an associator. It names the three crates that own them rather than carrying its own. **All three were already beneath this crate** through `gungnir-track` → `gungnir-association` → `gungnir-filters` → `gungnir-core`, so the graph gains no reach and no depth; what changed is that the manifest now says what the code imports. The refused alternative was a filter inside `gungnir-fusion-async`, which would have put a second Kalman update in the workspace beside the signed one | `dependency_graph.rs`; `gungnir-fusion-async/tests/oos_convergence.rs` |

One dev-only edge was added and is not drawn, per the convention for `gungnir-scenario`:
`gungnir-tracking-service` dev-depends on it for the §2 whole-pipeline replay row, which
names the five scenarios. `ARCHITECTURE.md` §7.1's list of dev-dependents was updated in
the same change.

## 13. Edge (s) -- **accepted by the owner as engineering reviewer, 2026-09-06**

`gungnir-remote` to `gungnir-interop`, for DN-25's outbound Cursor-on-Target sink. Accepted
ahead of the code rather than with it, because DN-25 is a design-only note and it had to say
which crate owns the sink before it could say anything else about it.

| Edge | What it is | Evidence |
|---|---|---|
| (s) remote → interop | The sink encodes a view as an SD-16 payload and writes it to a socket. `endpoint.rs` and `peer.rs` already own outbound connections, their retries and their refusal counts, and DN-07's handoff proved that is where an outbound path belongs. The refused alternative was to leave encoding in `gungnir-interop` and give the socket to each host binary, the shape the ASTERIX adapter has: that would put a socket, a retry loop and a refusal count into both `gungnir-app` and `gungnir-node`, duplicating what one crate already has, and it was refused for that reason rather than on layering grounds | `DN-25-cursor-on-target.md` §2, §4 |

**Direction and depth.** The same direction as edge (j), `gungnir-ingest` to
`gungnir-interop`, in a manifest since 2026-09-06. `gungnir-interop` depends on
`gungnir-model` alone and `gungnir-remote` already depends on `gungnir-model`, so the edge
adds no reach below what `gungnir-remote` can already see, and it **cannot create a cycle**:
nothing in `gungnir-interop`'s subtree can reach `gungnir-remote`.

**Not in a manifest, and not drawn in `ARCHITECTURE.md` §7.1.** No code needs it yet.
Acceptance and existence are different things, and §4a's row says which this is. The graph in
§7.1 describes manifests; an edge drawn there that no manifest carries would be the graph
claiming something untrue, which is the one thing that document may not do. The change that
adds the sink adds the manifest line and draws (s) in the same commit, per §5's rule.

## 14. Edge (t) -- `gungnir-remote` to `gungnir-security` (recorded 2026-09-07; GAP-060, D-29)

Recorded after the fact. The edge was in the manifest and carrying working code; what was
missing was this entry. §5's rule is that a manifest line and its entry land together, and
here they did not.

| Edge | What it is | Evidence |
|---|---|---|
| (t) remote → security | `identity.rs` builds a host's TLS identity from that host's own `KeyProvider`, so **the private half never leaves custody**: `rcgen` signs through the provider's `sign`, rustls presents the result over the same call, and no path in the module can produce a certificate over key material that has left a provider. It imports `KeyId`, `KeyProvider`, `KeyPurpose` and `SignatureScheme`, and nothing else. **The refused alternative was `gungnir-api`**, the better home on layering grounds, rejected because `gungnir-app` holds it as a **dev-dependency only** and `ARCHITECTURE.md` refuses a dev-dependency as a production edge. Leaving the code in `gungnir-node`, where it lived until 2026-09-06, was refused separately: the desktop needs the same identity (GAP-060), two binaries cannot depend on each other, so it would have meant roughly 150 duplicated lines with nothing holding the copies in step | `gungnir-remote/src/identity.rs`; `DN-22-key-management.md` amendment 1; D-29 |

**Why `gungnir-remote` carries it.** It is the only crate **both binaries already depend on
at runtime** that already holds `rustls`, `tokio-rustls` and `rustls-pemfile`, and it already
owns `LinkTls` -- the type that answers *who is this host*. The two alternative homes are the
ones ruled out above.

**Direction and depth.** `gungnir-remote` is Deployment and `gungnir-security` is
Productization (`dependency_graph.rs`), so this is downward, the same shape as (p) and (s).
It **cannot create a cycle**: `gungnir-security` has no `gungnir-*` dependency at all, so
nothing in its subtree can reach `gungnir-remote`.

**How it went unrecorded, which is the part worth keeping.** The edge arrived on 2026-09-06
with the move of `identity.rs` out of `gungnir-node`, and the module's own documentation
carries the whole argument above -- the reasoning was never lost, only the register entry.
`ARCHITECTURE.md`'s §7 dependency table did not list it, and its §7.1 entry for (p) named it
only in passing, inside a different edge's justification, where nothing would look for it.
`dependency_graph.rs` did not catch it either, because that test checks layer **direction**
-- deployment down to productization, which this is -- and not individual edges, so an
unlisted edge in a legal direction passes silently.

It surfaced on 2026-09-07, when the UAF generator ran in CI for the first time (GAP-061,
"release workflow unexercised") and the view it regenerated from the manifests showed an edge
the table did not.

**That gap is closed the same day.** `gungnir-app/tests/dependency_graph.rs` gained
`the_manifests_and_architecture_md_agree_on_every_edge`, which parses §7's table and §7.1's
graph back out of `ARCHITECTURE.md` and compares both against the manifests, in both
directions: an edge in a `Cargo.toml` that the document does not list, an edge the document
lists that no manifest carries, and a crate documented in neither place. Only production
`[dependencies]` are compared, because both documents mark dev-only edges separately.

It was checked against the fault it exists for, rather than only against a passing tree:
deleting `security` from the `gungnir-remote` row reproduces this entry's finding and fails
the test by name. It also found one further disagreement on its first run -- §7's table
listed `testkit` for `gungnir-oracle` as though it were a production dependency when it is a
`[dev-dependencies]` entry, which the table now marks `(dev: ...)` the way §7.1 already
marked `gungnir-collab`'s and `gungnir-mission`'s.

## Traceability

The five notes that add edges: DN-03, DN-11, DN-12, DN-13, DN-19. The four that refuse
one: DN-01, DN-06, DN-09, DN-15. `../../ARCHITECTURE.md` §7.1;
`../agentic-coding-standards.md` §1.1; principle AP-10; contract C-11; GAP-081.

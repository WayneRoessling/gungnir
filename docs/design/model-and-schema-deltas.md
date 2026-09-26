# Canonical model and schema deltas

Status: first draft, 2026-09-05. Every change the design set makes to `gungnir-model` and
to the configuration baseline, consolidated so the canonical model is reviewed once.

This document exists because twenty-two separate changes to the type everything depends on
is how a canonical model stops being canonical.

## 1. New types in `gungnir-model`

| Type | Note | Why the model owns it |
|---|---|---|
| `AssetId`, `AssetPriority`, `AssetExtent`, `WarningObligation`, `DefendedAsset`, `AssetListView` | DN-01 | Assessment, configuration, reporting, the interface, and the viewport all need them |
| `EffectorLayer`, `RelativeCost`, `Magazine` | DN-04 | Configuration writes them, allocation reads them, the interface publishes them |
| `PolicySettings` and its six sub-types | DN-08 | Configuration deserializes them; policy and command read them and may not depend on configuration |
| `WeaponsControlStatus` | DN-09 | Carried in policy settings and published on the snapshot |
| `PlanKind`, `FiresPlan`, `DeconflictionResult`, `DeconflictionCheck`, `DeconflictionKind` | DN-05 | The plan is the canonical recommendation type |
| `DecisionId` | DN-06 | Lets a service facade key on a decision without depending on the crate that records decisions |
| `CollectionRequirement`, `RequirementState`, `RequirementId` | DN-11 | Workflow states them, sensor management serves them, the interface publishes them |
| `Measurement` (replacing `DetectionView.measurement`) | DN-27, proposed 2026-09-06; **built 2026-09-06**, and `SCHEMA_VERSION` is now 3 | **Breaking, and there is no smaller change.** An optional range beside a position would let a producer set both or neither and force the reader to guess. Three variants -- position, range/azimuth/elevation, and bearing -- each carrying its own error, because a bearing's angular error *is* its information and a fixed positional variance is wrong at every range but one. `elevation_rad` is `Option` because a missing elevation is not a zero one: zero means the horizon, and an optional that serialised indistinguishably from zero would put every acoustic detection there. **`SCHEMA_VERSION` must bump**; the exact-match version rule will then refuse a peer one version out, which is the correct outcome |
| `IdentityEvent` | GAP-019, 2026-09-06 | What a track was taken to be across sessions, and why. It exists because the answer had nowhere to go: the desktop kept lineage in memory for one panel and journalled none of it, and `TrackView` carries no `GlobalEntityId`. **Both outcomes are recorded**, the joins and the mints, because a reviewer asking why two sightings were not joined needs the mint too; and a correlation carries its **basis** as well as its confidence, since a number alone is an assertion. Additive, so `SCHEMA_VERSION` is unchanged. **`TrackView` was deliberately not given an entity identity**: that changes what every consumer of a track believes it is holding, and the record does not need it |
| `CalibrationEvent` | GAP-014, 2026-09-06 | Registration evidence is journalled, and the journal is typed by the model. It carries **both** the surveyed truth and the sensor's report rather than a pre-computed residual, because a residual cannot be re-examined when a survey is later corrected, and surveys are corrected. Its refusal variant exists because a deployment that gathered evidence and could not use it needs to know that. **The tracking-core consumer takes a plain-data equivalent instead** (`gungnir_track_fusion::ReferenceObservation`): a core crate consuming a model event would be an upward edge, so `gungnir-tracking-service` joins the two |
| `MissionProfile`, `AlgorithmBaselineId` | DN-24 | Configuration declares them, `gungnir-modelops` keys its registry on them, the binaries journal them, and `Provenance` will carry one once the pipeline applies it (implemented 2026-09-05; the model edge this forced on `gungnir-modelops` is edge (h) in `ARCHITECTURE.md` §7.1) |
| `looks_like_key_material` | DN-22 | A function rather than a type, and here for the same reason: `gungnir-config` validates baselines with it and `gungnir-security` owns custody, and a foundational crate must not depend on a productization one (moved 2026-09-05 with GAP-084) |
| `SensorTaskId` | DN-11 | The event schema carries it, and the model cannot depend on the crate that issues the tasks; `gungnir-sensor-management` re-exports it (added 2026-09-05 with GAP-004) |
| `Concurrence` | DN-11 amendment 1 (b) | Who concurred with tasking a requirement, distinguishing an attributed operator from a role that acted with no operator session (GAP-057). Workflow writes it, the interface publishes it (added 2026-09-05 with GAP-005) |
| `PeerOrigin` | DN-16 | Attaches to `Provenance` |
| `RehearsalOrigin` | DN-32 §6; GAP-105, 2026-09-25 | Attaches to `Provenance`: which recording, which laydown and which seed a re-observed detection came from, so the gateway, the exchange and a reader of the record can each tell it from a sensor's. `gungnir-sensor-sim` sits beneath the model and marks its observations with its own `SimulationMark`; the desktop converts |
| `Releasability` | DN-17 | Every markable type carries it |
| `Handoff`, `DecisionAttribution`, `EffectorReport` | DN-07 | The journal records them and the interface carries them |
| `ExchangeAgreement`, `ExchangeItem`, `ExchangeFormat` | DN-18 | Configuration writes them, the interface enforces them |

## 2. Changes to existing model types

| Type | Change | Additive? | Note |
|---|---|---|---|
| `ResourceView` | Gains `layer`, `cost`, `magazine` | Yes | DN-04 |
| `Provenance` | Gains `peer: Option<PeerOrigin>` | Yes | DN-16 |
| `Provenance` | Gains `rehearsal: Option<RehearsalOrigin>`, defaulted when absent and left out when `None` | Yes: every existing record, journal and fixture reads and is written byte for byte as before, so `SCHEMA_VERSION` is unchanged | DN-32 §6 mechanism 1; GAP-105. A live `IngestGateway` refuses a detection carrying it; the Arrow exchange refuses rather than strips it |
| `TrackView`, `PlanView`, `MissionReport`, `Handoff`, `Anomaly` | Gain `releasability` | Yes, defaulting to `Internal` | DN-17 |
| `CommandEvent::Decided` | Gains `decision: DecisionId` | Yes | DN-06 |
| `CommandEvent::Decided` | Gains `role: Option<String>`, the signed-in account's role, `None` with nobody signed in | Yes, defaulted, so an older journal reads | D-53 (2026-09-16): the arbitration rule needs a role to rank |
| `LinkEvent` | Gains `ConflictArbitrated { plan, kept_local, ground, local, remote, at }` | Yes | D-53: the rule's verdict on a reconciliation conflict, distinct from a person's `ConflictResolved` |
| `CommandEvent` | Gains `Expired` and `Escalated` variants | Yes | DN-10 |
| `Event` | Gains `Warning`, `Engagement`, `SensorTask`, `Handoff`, `Review`, `Handover` variants | Yes | DN-03, DN-06, DN-07, DN-11, DN-20, DN-21 |
| `Event` | Gains a `Requirement` variant | Yes | **Not in DN-11 §6.** Added 2026-09-05 with GAP-005 because CAP-2.12's method is an MT-08 replay and a lifecycle that never reaches the journal cannot be replayed. DN-11 amendment 1 (a) |
| `RequirementEvent::Stated` | Carries the whole `CollectionRequirement` rather than its title | **No**, and corrected before anything depended on it | An event that cannot reconstruct what it describes cannot be replayed; the first version made a rebuild impossible (fixed 2026-09-05 with GAP-005) |
| `CollectionRequirement` | Gains `priority: AssetPriority` | Yes, defaulted | DN-11 §3 shows it; the first implementation omitted it (added 2026-09-05) |
| `SnapshotResponse` | Gains `requirements` | Yes, defaulted | DN-11 §6 (added 2026-09-05) |
| **`PlanView.solutions`** | **Replaced by `PlanView.kind: PlanKind`** | **No** | DN-05 |
| **`DecisionId`, `PlanId`** | **A `u64` counter becomes a UUID v7 held as `u128`**, written as the hyphenated RFC 9562 string and read from that string or a number | **No**; a journal holding the old numbers still reads | D-56, D-60; GAP-130, `DN-31-node-approval-queue.md` §5.1 and amendment 1 |
| **`gungnir_command::PendingApprovalId`** | **The same change**, through the same helper, `gungnir_model::identifier` | **No** | D-56, D-60; GAP-130 |
| `PendingApprovalId` | **Moves to `gungnir-model`**; `gungnir-command` re-exports it and still mints it | Yes -- the same type, the same written form, the same tag | GAP-132: `CommandEvent::Queued` names the item a node queued, and the model may not depend on the crate holding the queue (DN-31 §5.3) |
| `RequestId` | New: a client's idempotency key for one decision, a validated non-empty bounded string. **Not a UUID under D-60**, because the client chooses it rather than this deployment minting it | Yes, new | DN-31 §5.2; GAP-132 |
| `CommandEvent::Decided` | Gains `request: Option<RequestId>` and `origin: Option<String>` | Yes, both defaulted, so an older journal reads | DN-31 §5.3; GAP-132. `origin` is filled by GAP-134 and is `None` for every decision taken here |
| `CommandEvent` | Gains `Queued { item, plan, layer, offered_to, expires_at, escalate_at }` | Yes -- a new variant, and a client must ignore an unknown one (`gungnir-api-v1.md`, "Compatibility rules") | DN-31 §5.3; GAP-132: a desktop builds the node's queue from the stream without polling |
| `gungnir_command::DecisionRecord` | Gains `item: Option<PendingApprovalId>`, `request: Option<RequestId>` and `origin: Option<String>` | Yes, all defaulted | GAP-132: what became of an item, and what a request key already produced, are answered from the append-only history rather than an index beside it (DN-31 §6.3) |
| `SnapshotResponse` | Gains `queue: Vec<QueueItemView>` | Yes, defaulted | DN-31 §5.3; GAP-132 |
| `gungnir-api` v3 | New: `QueueItemView`, `DecisionRequest`, `DecisionChoice`, `DecisionRecorded`, `DecisionRefused` | Yes, new | DN-31 §5.2; GAP-132 |
| `CommandEvent` | Loses `ApprovalRequested(PlanId)`, which nothing published | **No**, and no journal holds one | DN-31 §5.3 and amendment 1 |
| **`SCHEMA_VERSION`** | **3 becomes 4**, and the interface path `/v2` becomes `/v3` | **No** | D-56; every `/v2` route answers `410 Gone` naming its successor |
| `LinkEvent` | Gains `BothActed { track, local, remote, at }`, each side an `EngagementSide { decision, plan, at }` beside `LinkEvent` rather than in `arbitration`, because nothing ranks the two | Yes -- a new variant | D-58, DN-31 §5.3; GAP-134: two engagements of one track across an outage, for a person, whatever any verdict said |
| `LinkEvent` | Gains `DelegationsLapsed { endpoint, cut_off_since, lapse_s, withdrawn, at }` | Yes -- a new variant | D-15, DN-31 §6.7; GAP-134. **Not in DN-31 §5.3's list**: a lapse changes what queued items are actionable by, and the record names the ones it withdrew |
| `PolicySettings` | Gains `delegation: DelegationSettings { disconnected_lapse_s: Option<f64> }` | Yes, defaulted -- to silence, which is no interval rather than a default one | D-15, DN-31 §7; GAP-134 |
| `gungnir-api` v3 | New: `DecisionRecordView`, `ForwardedDecision { record, origin, settled }`, `Settlement`, `ForwardAccepted`, `ForwardRefused` | Yes, new. **`settled` is beyond §5.2's two fields**, defaulted, and carries §6.8's forwarded verdicts on the one route §7 gives | DN-31 §5.2, §6.8, §7; GAP-134 |
| `gungnir_command::ApprovalWorkflow` | Gains `admit_forwarded`, keyed on the `DecisionId` and idempotent on it, and `reoffer` for a lapse | Not a wire change | DN-31 §6.7, §6.8; GAP-134 |

## 3. The one breaking change

`PlanView.solutions: Vec<InterceptSolutionView>` becomes `PlanView.kind: PlanKind`. The
interface's own compatibility rules say that changing a field's type requires a new schema
version and a new path version.

**Decided by the owner on 2026-09-05: option B, replace it outright.**

| What changes | To |
|---|---|
| `gungnir_model::SCHEMA_VERSION` | 1 becomes 2 |
| The interface path | `/v1` becomes `/v2` |
| `PlanView.solutions` | Removed. No deprecated mirror is kept |

The option not taken was to keep `solutions` as a deprecated mirror alongside `kind`. It
was rejected because two fields saying overlapping things is a permanent drift risk bought
for a temporary benefit, and the benefit is zero here: there is no deployed client to
protect.

**This is consistent with the contract's own versioning rule rather than an exception to
it.** `../gungnir-api-v1.md` says `v2` is added alongside `v1` and `v1` is removed only
after every known client has moved. The set of known clients is empty, because the
transport is not in the workspace and `ApiServer::serve` still returns a not-implemented
error. The condition the rule names is met by inspection.

**Landed 2026-09-05**, all three parts in one change as intended: the type, the schema
version, and the module path. Ten call sites across six crates moved from the field to
`PlanView::solutions()`, which returns the intercept solutions and is empty for a fires
plan. `PlanView::assignments` now covers fires by yielding its firing unit and target, so
every policy that walks assignments handles both without a second code path.

## 4. Configuration baseline sections

New sections and fields, all defaulting so that `SUPPORTED_CONFIG_VERSION` stays 1, with
one exception.

| Section or field | Note | Default |
|---|---|---|
| `assets: Vec<AssetConfig>` | DN-01 | Empty |
| `endpoints: Vec<EndpointConfig>` | DN-03 | Empty |
| `resources[].layer` | DN-04 | **No default. A baseline without it is rejected** |
| `resources[].cost`, `rounds_available`, `reserve` | DN-04 | Absent |
| `resources[].handoff_endpoint` | DN-07 | Absent, meaning manual delivery |
| `policy: PolicySettings` | DN-08 | `Default::default()`, which is the strictest reading |
| `validity: Option<ValidityWindow>` | DN-08 | Absent, always valid |
| `assessment.prediction_horizons_s` | DN-02 | A short set |
| `assessment.effect_window_s` per layer | DN-06 | Defaulted |
| `sensors[].control_endpoint` | DN-11 | Absent, not controllable |
| `sensors[].maintenance` | DN-21 | Empty |
| `sensor_task_ack_window_s` | DN-11 | Defaulted |
| `security.key_provider` | DN-22 | Absent means no custody, so nothing is encrypted and the system says so. Validation refuses a value that looks like key material (added 2026-09-05 with GAP-084) |
| `analytics.coverage_sample_spacing_m` | DN-12 | Defaulted |
| `analytics.max_sensor_plan_candidates` | DN-13 | Defaulted |
| `analytics.anomaly: AnomalySettings` | DN-15 | Empty, meaning every detector off and reported as off |
| `hazards: Vec<HazardConfig>` | DN-14 | Empty |
| `peers: Vec<PeerConfig>` | DN-16 | Empty |
| `exchange: Vec<ExchangeAgreement>` | DN-18 | Empty |
| `default_releasability` | DN-17 | `Internal` |
| `reporting.scheduled`, `reporting.retention_sessions` | DN-19, DN-21 | Empty, defaulted |
| `security.key_provider` | DN-22 | Required when encryption is enabled |
| `radar_feeds: Vec<RadarFeedConfig>` (GAP-001, 2026-09-06) | handoff §4 step 1 | Empty; a feed with an unknown sensor, a duplicate SAC/SIC, or an unparseable address is rejected |
| `terrain: Option<TerrainConfig>` (GAP-023, 2026-09-06) | -- | Absent, flat line of sight; only `frame: "local-enu"` is accepted |
| `security.tls.trust_roots_pem` (GAP-060, 2026-09-06) | DN-22 | Empty, the platform's store; a private key is refused |
| `security.escrow: Option<EscrowConfig>` (GAP-084, 2026-09-06) | DN-22 §11 | Absent, no escrow; the public half only, a holder that names somebody |
| `peers: Vec<PeerConfig>` (GAP-009, 2026-09-06) | DN-16 | Empty; source ids that are no sensor's, quality in 0..=1, positive age |
| `assets[].warning_within_m` (GAP-042, 2026-09-06) | DN-03 amendment 1 | Absent, impact only; needs the lead time and channel, and must be positive |
| `security.key_provider: "passphrase-sealed-file"` (GAP-084, 2026-09-06) | DN-22 amendment 3 | A value, not a section: the keystore file lives beside the journal and opens with the operator's passphrase at sign-in |
| `misb_feeds: Vec<MisbFeedConfig>` (GAP-099, 2026-09-08) | external standards §8 | Empty, no ISR platform telemetry; a feed name or a receiver claimed twice, a sensor that is not in the sensor list, an unparseable `ip:port`, an empty recording path |
| `radar_feeds[].df_sites: Vec<DfSiteConfig>` (GAP-100, 2026-09-08) | external standards §9.1 | Empty, every Category 205 report counted `unknown_radar`; SAC/SIC and the sensor named for position, so an unknown sensor, a SAC/SIC or a sensor bound twice, and an `azimuth_sigma_rad` that is not finite and positive are all rejected -- never defaulted, since edition 1.0 carries no usable angular error on the wire |
| `radar_feeds[].uas_sites: Vec<UasSiteConfig>` (GAP-101, 2026-09-08) | external standards §9.3 | Empty, every Category 129 report counted `unknown_radar`; SAC/SIC and the sensor it takes its identity from, no position, so an unknown sensor or a SAC/SIC or a sensor bound twice is rejected. **`00`/`00` is accepted**, being the specification's own recommended pair for an airborne-to-ground broadcast; it is the duplicate that is refused, not the placeholder. **A feed binds a radar, a direction finder or a UAS gateway**; one that binds none of the three is rejected |
| `policy.delegation.disconnected_lapse_s` (GAP-134, 2026-09-17) | DN-31 §7, D-15 | **None, and absent is not a default interval**: a desktop cut off under a baseline that states none holds no delegation from the moment it falls back. Stated, it must be finite and positive, or the baseline is refused |
| `sensors[].detection_model` (GAP-105, 2026-09-25) | DN-32 §5.4 | **Absent, and absent means the sensor cannot be rehearsed**: a laydown rehearsal refuses by name a laydown that places it in an observing mode, and never infers a model from `modality` and `max_range_m` or borrows a recording's sensor of the same identifier. Present, it names a sensor type in `testdata/tracks/sensor-models.json`; an empty or spaced name is refused at load, an unknown one by the rehearsal that reads the catalogue |

**`resources[].layer` is the only mandatory addition.** It is mandatory because MOE-03 is
defined by it, and defaulting it would silently corrupt the product's headline measure.

**The binding rule is now a three-way "or", and that is the only thing GAP-100 and
GAP-101 changed about an existing field.** A feed exists to carry something; one that
names no radar, no direction finder and no UAS gateway is refused rather than given a
socket. Each of the three lists still defaults to empty on its own, and an empty one
means its category is counted `unknown_radar` rather than guessed at.

## 5. The defaults doctrine

Read across the table, the defaults follow one rule with one deliberate exception:

- **Silence about authority denies.** No control status means `Hold`; no authority rule
  means denied; no releasability means `Internal`; no identification threshold means a
  person confirms.
- **Silence about capability disables and says so.** No adapter means not controllable; no
  detector settings means the detector is off and reported off; no endpoint means manual
  delivery.
- **The exception: silence about expiry preserves.** No expiry configured means a pending
  decision does not expire, because silently discarding a decision nobody took would lose
  information. It is visible in the queue.

Both kinds of error are visible to the operator. Only the first kind would be unsafe.

## 6. New authorization actions

Added to `gungnir_security::actions`, each governed by the authority matrix:

| Action | Note |
|---|---|
| `weapons.control_status` | DN-09 |
| `effector.report` | DN-07 |
| `product.release` | DN-17 |
| `review.conduct` | DN-20 |
| `handover.acknowledge` | DN-21 |

`sensor.task` already exists and DN-11 and DN-13 reuse it rather than adding a near
duplicate.

## 7. Interface endpoints added

All additive except where noted. **They land under `/v2`**, because option B moves the
path version with the model change and the two arrive together.

| Method and path | Note | Action |
|---|---|---|
| `POST /v2/sensors/{id}/task` | DN-11 | `sensor.task` |
| `GET /v2/coverage` | DN-12 | `picture.view` | Built 2026-09-05 (GAP-006), returning the whole `CoverageReport` rather than the bare gap list §6 wrote |
| `GET /v2/sensor-plans` | DN-13 | `sensor.task` |
| `POST /v2/handoffs` | DN-07 | `plan.decide` |
| `POST /v2/handoffs/{id}/report` | DN-07 | `effector.report` |
| `GET /v2/order-of-battle`, `GET /v2/pattern-of-life` | DN-19 | `picture.view` |
| `POST /v2/reviews`, `/findings`, `/state`, `GET /v2/reviews` | DN-20 | `review.conduct`, `picture.view` |
| `GET /v2/handover`, `POST /v2/handover/acknowledge` | DN-21 | `picture.view`, `handover.acknowledge` |
| `GET /v2/history?since_seq=N` | GAP-050 (2026-09-06) | `picture.view` | Built: the retained window from `N`, `410 Gone` past it |
| `POST /v3/decisions/forwarded` | DN-31 §7, GAP-134 (2026-09-17) | `plan.decide` | Built: a batch taken whole or not at all; `202` counted, `409` naming what stands. New in `/v3`, so it has no `/v2` form |

**Landed 2026-09-06**: `Event` gained `Engagement(EngagementEvent)` and
`Review(ReviewEvent)`, with the engagement outcome vocabulary in
`gungnir_model::events::engagement_outcome` so the report can count evidence sources
apart without an edge to the facade; `MissionReport` gained `measures: Vec<Measure>`;
`ConfigBaseline` gained `hazards` and `analytics.max_sensor_plan_candidates`.
`SnapshotResponse.hazards` was not added: it would need an edge from `gungnir-api` to
`gungnir-geo` that nothing has argued. Later the same day: `ConfigBaseline` gained
`revision` (per-promotion; `apply` refuses it unadvanced), `CommandEvent::Decided` gained
`verdict: VerdictSummary` and `rationale: Option<String>`, and `Event` gained
`Health(HealthEvent)` and `Replay(ReplayEvent)`; `Measure` gained `note`.

**Landed 2026-09-06, batch 6**: `WarningObligation` gained `within_m: Option<f64>`
(DN-03 amendment 1); `LinkEvent` gained `SwitchedBack` (GAP-050); `TerrainMesh` in
`gungnir-data` gained `rows` and `columns` (GAP-023); `AisMessage` and its eight typed
messages entered `gungnir-interop` (D-32), owned there because no other crate reads them
yet.

**Landed 2026-09-06, batch 7**: `ConfigBaseline` gained `ais_feeds: Vec<AisFeedConfig>`
(`AisSource::{Tcp, File}`), `exchange: Vec<ExchangeAgreement>` (validated against the peers
and endpoints), and `reporting.retention_sessions` (default 10); `SnapshotResponse` and
`HistoryResponse` gained `withheld: usize` (DN-17 increment 4); `LinkEvent` gained
`ConflictResolved { plan, kept_local, operator, at }` (GAP-050); `SchemaKind` gained
`Ais { edition }` and the catalogue the `ais.m1371` entry; the Arrow detection schema
gained `source_sensor_ids_json`, `calibration_baseline_version`, `authentication`,
`peer_json` and `conversion_loss` so the round trip is lossless (GAP-063);
`InteropError` gained `IncompatibleSchema` and `UnknownSchema`.

**Landed 2026-09-06, batch 8**: `SensorCommand` moved to `gungnir-model` (re-exported by
`gungnir_sensor_management::tasking`) so `POST /v2/sensors/{sensor_id}/task` can carry it
(`SensorTaskRequest`, `SensorTaskResponse`); `HandoffEvent` gained `Reported { decision,
endpoint, report, at }` and the contract `POST /v2/handoffs/{decision_id}/report`
(`EffectorReportRequest`); `ConfigBaseline` gained `machine_identities`
(`MachineRole::{Sensor, Effector, Peer}`) and `assessment.lethality_by_class`;
`AssetExposure` gained `time_to_closest_approach_s`; the catalogue gained
`gungnir.ml.classification-rows` version 1 (`gungnir_interop::dataset`);
`RemoteEndpoint` gained `tls: LinkTls`.

**Landed 2026-09-16, the GAP-067 walk**: `gungnir_model::arbitration` holds D-03's rule
over the facts it reads (`ArbitrationFacts`, `Verdict`, `ArbitrationGround`, `ConflictSide`,
`SideOutcome`), and `Resolution` moved there from `gungnir-collab`, which re-exports both,
because no binary depends on `gungnir-collab` and the desktop's reconciliation applies the
rule (D-53). `DecisionRecord` in `gungnir-command` gained `role`, and
`ApprovalWorkflow::decide` takes it; `gungnir_resilience::DecisionConflict` carries both
sides as `ConflictSide` in place of two booleans. `SCHEMA_VERSION` is unchanged: every
addition reads an older journal.

**Landed 2026-09-17, GAP-130**: `DecisionId`, `PlanId` and `PendingApprovalId` are UUID v7
held as `u128`, minted where each thing is created -- `gungnir-command`'s workflow for
decisions and queue items, the intercept service's planner for plans -- and written as the
hyphenated string, read from that string or a number, and shown by a short tag on screen,
all through `gungnir_model::identifier` (D-56, D-60, D-61). `SCHEMA_VERSION` is 4,
`CommandEvent::ApprovalRequested` is gone, and every interface path in this document is
served under `/v3`, with its `/v2` form answering `410 Gone` after authenticating the caller
as the successor does. `testdata/journals/pre-uuid-v7/` holds a journal written at version
3, and it replays and reports unchanged.

**Landed 2026-09-17, GAP-132**: the node runs the approval queue (D-55), so the queue is on
the wire. `PendingApprovalId` moved to `gungnir-model` -- unchanged in shape, written form
and tag, and still minted by `gungnir-command`, which re-exports it -- because
`CommandEvent::Queued` names the item and the model may not depend on the crate holding the
queue. `CommandEvent` gained that variant and `Decided` gained `request` and `origin`;
`DecisionRecord` gained `item`, `request` and `origin`; `SnapshotResponse` gained `queue`;
and `gungnir-api` v3 gained `QueueItemView`, `DecisionRequest`, `DecisionChoice`,
`DecisionRecorded` and `DecisionRefused`. **`SCHEMA_VERSION` stays 4**: every addition is a
defaulted field or a new enum variant, which the interface's own compatibility rules call
compatible, and every payload that already read still reads. `ApprovalWorkflow::decide`
takes a `DecidedBy` in place of two `Option<String>`s, so the operator, the role and the
request key of one act cannot be passed separately and disagree.

**Landed 2026-09-17, GAP-134**: an outage's decisions reach the node. `LinkEvent` gained
`BothActed` (D-58), with its sides as `gungnir_model::events::EngagementSide`, and
`DelegationsLapsed` (D-15); `PolicySettings` gained `delegation`, whose one field
`disconnected_lapse_s` has no default; `gungnir-api` v3 gained the forwarded route's types,
`ForwardedDecision` carrying a defaulted `settled` beside DN-31 §5.2's `record` and
`origin`; and `gungnir_resilience::ReconcileReport` gained `both_acted`. `CommandEvent::Decided`
already carried `origin` and now a forwarded decision fills it. **`SCHEMA_VERSION` stays 4**
for the reason GAP-132's additions left it there.

`SnapshotResponse` gains `assets`, `predictions`, `engagements`, `requirements`,
`hazards`, and `control_status`, and is filtered per caller by DN-17. The filtering is a
behaviour change on an existing endpoint and is gated on the caller having a configured
party, so a deployment with no parties sees no change.

## Traceability

Every design note in this folder; `../gungnir-api-v1.md` for the compatibility rules;
`../../ARCHITECTURE.md` §7.2 for the model's ownership rule; principles AP-06, AP-09;
contracts C-07, C-08, C-10.

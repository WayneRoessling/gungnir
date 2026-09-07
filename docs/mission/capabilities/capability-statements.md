# Capability statements

Status: first draft, 2026-09-04. One entry per leaf in `capability-taxonomy.md`.
Maturity per increment uses three levels: **none**, **partial** (the outcome is
achievable with workarounds or for a subset of cases), **full** (the outcome is
achieved to its measures). Increments are those in `../../gungnir-capabilities.md`
§7: I1 productize the core, I2 integrate real data, I3 close the decision loop, I4
operationalize and scale. Measures are defined in `measures-catalogue.md`; crate
status comes from `../../gungnir-capabilities.md` §8. "Considerations" lists what the
capability needs beyond software (doctrine, organization, training, materiel,
leadership, personnel, facilities, policy).

Every target value and maturity level is a proposal for the owner to confirm.

Format:

```
CAP-x.y  Name
- Statement: the force shall be able to ...
- Rationale: why, from the threads.
- Measures: ids with targets.
- Maturity: I1 / I2 / I3 / I4.
- Threads: MT-xx.
- Provided by: crates (status).
- Considerations: what the capability needs beyond software.
```

---

## CAP-1 Sense

**CAP-1.1 Ingest observations from every sensor class**

- Statement: the sector shall bring observations from radar (long, medium, short
range, ground, coastal), electro-optical and infrared, acoustic arrays, radio-
frequency detectors, ISR video, cooperative sources, and spotter reports into one
canonical observation stream carrying source, source time, receipt time, and
calibration baseline.
- Rationale: every thread begins with detection from a mosaic of sensors of different
rates and reliabilities (MT-01, MT-02, MT-04, MT-06).
- Measures: MOP-17 (5,000 detections per second per node), MOP-19 (source sensor, source time, and receipt time on every accepted observation: 1.0; calibration baseline optional).
- Maturity: I1 partial (recorded and simulated feeds) / I2 full for the sensors in the
lead mission / I3 full / I4 full.
- Threads: MT-01 to MT-08.
- Provided by: `gungnir-ingest` (gateway implemented; recorded and simulated
adapters; live adapters pending), `gungnir-interop` (codec boundary), `gungnir-model`
(`DetectionView`).
- Considerations: sensor procurement and data-link availability (materiel);
interface agreements with sensor owners (policy).

**CAP-1.2 Validate, authenticate, and quarantine input**

- Statement: the sector shall reject or quarantine any observation or message that is
malformed, implausible, out of time bounds, or from an unauthenticated source before
it can affect the picture, and shall record what was quarantined and why.
- Rationale: the ingest gateway is the trust boundary for external data (MT-01, MT-05,
MT-07); malicious or broken feeds must not reach the tracking core.
- Measures: MOP-20 (zero quarantined-class inputs reaching the tracker under the
fuzz corpus and the validation tests), MOE-06.
- Maturity: I1 full for structural validation / I2 full with source authentication for
live adapters / I3 full / I4 full.
- Threads: MT-01 to MT-08, MT-10.
- Provided by: `gungnir-ingest` (implemented, tested, fuzzed), `gungnir-security`
(source authentication trait).
- Considerations: credential issuance to sensor owners (policy, organization).

**CAP-1.3 Manage sensor readiness, modes, tasking, and calibration**

- Statement: the sensor manager shall be able to see every sensor's state, change its
mode within the permitted transitions, task it against an area or a track, and
record its calibration baseline, with each change audited.
- Rationale: MT-03, MT-07, MT-08, MT-09 all turn on re-tasking and mode management.
- Measures: MOP-21 (mode change reflected in coverage within 10 s), MOP-14.
- Maturity: I1 partial (registry, modes, coverage regions) / I2 full for configured
sensors / I3 full / I4 full.
- Threads: MT-03, MT-07, MT-08, MT-09.
- Provided by: `gungnir-sensor-management` (implemented, not wired), `gungnir-config`.
- Design: `../../design/DN-11-sensor-control-and-tasking.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: sensor control interfaces (materiel); tasking authority (policy).

**CAP-1.4 Know sensor coverage and gaps over terrain**

- Statement: the sector shall be able to compute and display what each sensor can see
over terrain in its current mode, the combined coverage, and the gaps, especially
along low-altitude approaches, and recompute it when a sensor changes state.
- Rationale: laydown (MT-09) and degradation (MT-07) decisions depend on it.
- Measures: MOP-14 (under 10 s), MOE-10.
- Maturity: I1 partial (analytics and coverage regions; no map rendering) / I2 partial
/ I3 full / I4 full.
- Threads: MT-07, MT-09.
- Provided by: `gungnir-analytics` (implemented), `gungnir-sensor-management`,
`gungnir-viewport3d` (rendering pending), `gungnir-data` (terrain).
- Design: `../../design/DN-12-coverage-and-gaps.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: terrain data availability (materiel).

**CAP-1.5 Keep time discipline**

- Statement: every observation shall carry source and receipt time; late data shall be
handled by an explicit policy; clock skew across sources shall be detected and
reported; replay shall use recorded time.
- Rationale: out-of-sequence data is normal (MT-01, MT-06), GNSS denial corrupts
clocks (MT-07), replay must be deterministic (MT-09).
- Measures: MOP-09 (skew flagged within 30 s), MOP-15 (replay determinism).
- Maturity: I1 full for wall and replay clocks / I2 partial with skew tracking / I3
full / I4 full.
- Threads: MT-01, MT-06, MT-07, MT-09, MT-10.
- Provided by: `gungnir-time` (implemented; skew tracking pending),
`gungnir-fusion-async` (late-data handling pending).
- Considerations: time-source policy for sensors (policy).

**CAP-1.6 Receive early warning, tracks, and reports from peers**

- Statement: the sector shall receive launch warnings, tracks, and reports from higher
command and neighbouring sectors, with their provenance and staleness visible, and
merge them into the picture as low- or high-quality sources per policy.
- Rationale: minutes of warning for missiles and raids come from up-threat (MT-01,
MT-02).
- Measures: MOP-22 (peer track source shown: 1.0; latency and staleness where the peer supplies them).
- Maturity: I1 none / I2 partial (recorded peer feeds) / I3 partial / I4 full with the
API transport.
- Threads: MT-01, MT-02, MT-04, MT-05, MT-08.
- Provided by: `gungnir-api`, `gungnir-remote`, `gungnir-interop` (contract and
catalogue; transport pending).
- Design: `../../design/DN-16-peer-sources.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: interface control agreements with peers (policy, organization).

**CAP-1.7 Receive cooperative identity**

- Statement: the sector shall ingest IFF, ADS-B, AIS, and blue-force tracking and
attach the identity they carry to tracks as evidence, without treating their absence
as hostile evidence.
- Rationale: friendly and civil traffic share the space (MT-02, MT-05, MT-08); the
strongest positive identification is cooperative.
- Measures: MOP-23 (cooperative identity attached within two update cycles: p95).
- Maturity: I1 none / I2 partial (AIS or ADS-B decoder) / I3 full / I4 full.
- Threads: MT-02, MT-04, MT-05, MT-08.
- Provided by: `gungnir-ingest`, `gungnir-interop` (decoders pending),
`gungnir-identification` (evidence).
- Considerations: IFF key material and crypto (policy, materiel).

## CAP-2 Understand

**CAP-2.1 Maintain one multi-sensor track picture with uncertainty**

- Statement: for each domain the sector shall maintain a single set of tracks fused
from every source, each with a kinematic estimate and covariance, updated at the
rate the sources allow, consistent with the verified estimators.
- Rationale: the picture is the basis of everything (all live threads).
- Measures: MOP-01, MOP-03, and the tracking-core rows in
`../../verification-capability-table.md` §1.
- Maturity: I1 partial (facades and projection; pipeline unimplemented) / I2 full for
Scenarios 1 to 3 / I3 full / I4 full.
- Threads: MT-01 to MT-08.
- Provided by: `gungnir-tracking-service` (facade), `gungnir-fusion-async`,
`gungnir-filters`, `gungnir-association`, `gungnir-track`, `gungnir-track-fusion`
(trait surfaces).
- Considerations: none beyond software.

**CAP-2.2 Keep tracks through gaps with visible quality**

- Statement: tracks shall coast through dropouts with growing uncertainty, be marked
stale after a policy interval, be drawn distinctly when stale, and never be used
for allocation while stale.
- Rationale: horizon dropouts, cover, and jamming (MT-02, MT-06, MT-07); MOE-06.
- Measures: MOP-03, MOE-06.
- Maturity: I1 partial (quality fields, stale drawn muted, stale never allocated) / I2
full with the lifecycle implementation / I3 full / I4 full.
- Threads: MT-01, MT-02, MT-04, MT-06, MT-07.
- Provided by: `gungnir-track` (lifecycle pending), `gungnir-model` (`Quality`),
`gungnir-ui`, `gungnir-assessment`.
- Considerations: staleness policy per class (policy).

**CAP-2.3 Register sensors against each other**

- Statement: systematic position and orientation bias between sensors shall be
estimated and removed before fusion, and the calibration baseline recorded with the
tracks it affected.
- Rationale: MT-06 (Scenario 3 injected bias); a confident wrong fused position is
worse than none.
- Measures: the track-fusion registration row (bias recovered within 1e-3 of injected).
- Maturity: I1 none / I2 full for Scenario 3 / I3 full / I4 full.
- Threads: MT-06, MT-01.
- Provided by: `gungnir-track-fusion` (trait surface), `gungnir-data-fusion` (point-
cloud registration as evidence, pending).
- Considerations: survey of sensor positions (materiel, facilities).

**CAP-2.4 Track dense groups and count targets**

- Statement: in a raid or swarm the sector shall maintain the picture without
identity collapse and shall estimate the number of targets present when individual
tracks cannot be separated.
- Rationale: MT-01 raids of tens to over a hundred.
- Measures: the RFS rows (cardinality exact where unambiguous; weights within 1e-3),
MOP-16.
- Maturity: I1 none / I2 partial / I3 full for Scenario 4 / I4 full.
- Threads: MT-01, MT-02.
- Provided by: `gungnir-rfs` (trait surface).
- Considerations: none.

**CAP-2.5 Maintain a clutter-tolerant surface picture**

- Statement: the port and its approaches shall be tracked in sea clutter with a false-
track rate below the display threshold and without dropping a real low, fast craft.
- Rationale: MT-04, MT-05 (Scenario 2).
- Measures: MOP-04, MOE-07.
- Maturity: I1 none / I2 full for Scenario 2 / I3 full / I4 full.
- Threads: MT-04, MT-05.
- Provided by: `gungnir-association`, `gungnir-track` (trait surfaces),
`gungnir-scenario` (Scenario 2).
- Design: `../../design/DN-14-hazard-layer.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: none.

**CAP-2.6 Classify and identify with evidence and confidence**

- Statement: every track shall carry a classification (friend, hostile, neutral,
unknown) and, where evidence allows, a class, each with confidence and with the
evidence retained; a track shall stay unknown unless the evidence clears the policy
margin.
- Rationale: identification drives every engagement decision and every fratricide risk
(MT-01 to MT-04, MT-08).
- Measures: MOP-05, MOE-02, MOP-24 (evidence retained with every declaration: 1.0).
- Maturity: I1 partial (engine and margin rule) / I2 partial with cooperative and
kinematic evidence / I3 full with per-class policy thresholds / I4 full.
- Threads: MT-01 to MT-06, MT-08.
- Provided by: `gungnir-identification` (implemented), `gungnir-policy` (per-class
thresholds pending), plan 09 ML-01.
- Considerations: identification criteria (policy); operator training on evidence
(training).

**CAP-2.7 Keep one identity across gaps, sorties, sessions, and peers**

- Statement: an entity observed intermittently shall keep one global identity with an
inspectable lineage of the session tracks, merges, and splits that produced it.
- Rationale: MT-06 convoys under cover; MT-08 order of battle; MT-05 vessels across
days.
- Measures: MOE-09 (0.9 across scenario gaps).
- Maturity: I1 partial (session-track mapping, merges) / I2 partial / I3 full with
kinematic and class correlation / I4 full with peer identity exchange.
- Threads: MT-05, MT-06, MT-08.
- Provided by: `gungnir-identity` (implemented; cross-session correlation pending).
- Considerations: none.

**CAP-2.8 Predict trajectory, time to impact, closest approach**

- Statement: for every track the sector shall predict its path, the defended asset it
threatens, its time to impact, and for surface craft its closest point of approach
to protected ships and infrastructure.
- Rationale: prioritization and warning depend on it (MT-01, MT-02, MT-04).
- Measures: MOP-25 (prediction error against truth in test tracks: per class, set in the
plan 07 catalogue).
- Maturity: I1 partial (closing speed to one point) / I2 partial / I3 full against the
defended-asset list / I4 full.
- Threads: MT-01, MT-02, MT-04, MT-06.
- Provided by: `gungnir-assessment` (baseline implemented; asset list pending),
`gungnir-filters` (prediction).
- Design: `../../design/DN-02-prediction-and-approach.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: none.

**CAP-2.9 Detect anomalies in tracks and feeds**

- Statement: the sector shall flag tracks and feeds whose behaviour is inconsistent
with their declared identity or with normal traffic, as alerts through the lifecycle,
never as automatic quarantines or engagements.
- Rationale: MT-05 (AIS off, spoofing), MT-07 (a sensor reporting garbage).
- Measures: MOP-26 (anomaly flagged within two minutes in TT-05), MOP-18.
- Maturity: I1 none / I2 partial (rule-based) / I3 full / I4 full with learned
detection (plan 09 ML-04).
- Threads: MT-05, MT-07.
- Provided by: `gungnir-observability`, plan 09 `gungnir-ml`.
- Design: `../../design/DN-15-anomaly-detectors.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: none.

**CAP-2.10 Fuse terrain, imagery, point clouds, and map layers**

- Statement: the picture shall be shown against terrain, imagery, structures, and map
layers so operators orient at a glance, with point clouds from multiple sensors
registered into one surface where available.
- Rationale: situational awareness (all live threads); coverage over terrain
(CAP-1.4).
- Measures: MOP-16, the data-fusion rows.
- Maturity: I1 partial (loaders, 2D fallback) / I2 partial / I3 full with the three-d
scene and imagery layers / I4 full with point-cloud fusion.
- Threads: all live.
- Provided by: `gungnir-data`, `gungnir-data-fusion`, `gungnir-geo`,
`gungnir-viewport3d` (scene pending).
- Considerations: terrain and imagery data licensing (policy, materiel).

**CAP-2.11 Answer geometric questions**

- Statement: the sector shall compute line of sight, viewsheds, sensor coverage
volumes, and whether a route crosses a restricted area, from the terrain in use.
- Rationale: laydown (MT-09), degradation (MT-07), deconfliction (MT-06).
- Measures: the analytics rows (tested); MOP-14.
- Maturity: I1 full for the CPU reference / I2 full / I3 full / I4 full with GPU
variants where needed.
- Threads: MT-06, MT-07, MT-09.
- Provided by: `gungnir-analytics` (implemented).
- Considerations: none.

**CAP-2.12 Maintain pattern of life and order of battle**

- Statement: the sector shall accumulate adversary launch areas, routes, timings, and
unit locations across sessions, as entities with lineage and as analysis products,
and use them as priors for assessment and laydown.
- Rationale: MT-08, MT-09; engaging the ISR UAS pre-empts the strike.
- Measures: MOE-13 (products delivered within the battle rhythm: 0.95 of scheduled).
- Maturity: I1 none / I2 partial (replay and reports over journals) / I3 partial / I4
full.
- Threads: MT-08, MT-09.
- Provided by: `gungnir-identity`, `gungnir-replay`, `gungnir-reporting`.
- Design: `../../design/DN-11-sensor-control-and-tasking.md`, `../../design/DN-19-order-of-battle.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: analyst staffing (personnel).

## CAP-3 Decide

**CAP-3.1 Maintain the defended-asset list**

- Statement: the commander shall be able to define the assets protected, their
priorities and warning obligations, and change them with an audited, validated
baseline.
- Rationale: every score and every assignment is relative to it (MT-01, MT-02, MT-09).
- Measures: MOP-27 (priority change reflected in scoring within two ticks).
- Maturity: I1 none / I2 partial (one protected point) / I3 full / I4 full.
- Threads: MT-01, MT-02, MT-04, MT-09.
- Provided by: `gungnir-config` (asset list pending), `gungnir-assessment`.
- Design: `../../design/DN-01-defended-assets.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: the priority policy itself (leadership, policy).

**CAP-3.2 Score threat and priority per track**

- Statement: every track shall carry a threat score from its class, its predicted
asset, its time to impact, and the asset's priority, with the contributing factors
visible, and stale tracks scoring zero.
- Rationale: MT-01 saturation, MT-02 missiles first, MT-06 time sensitivity.
- Measures: MOP-28 (score monotonic in time to impact and asset priority on test
tracks), MOE-06.
- Maturity: I1 partial (closing speed) / I2 partial / I3 full with class lethality and
the asset list / I4 full with learned scoring in shadow mode (plan 09 ML-02).
- Threads: MT-01, MT-02, MT-04, MT-06.
- Provided by: `gungnir-assessment` (baseline implemented; not wired).
- Considerations: none.

**CAP-3.3 Recommend the cheapest adequate assignment**

- Statement: the sector shall recommend which resource engages which track so that
higher-priority threats are covered first, the cheapest adequate layer is used,
expensive interceptors are held for the threats that warrant them, unready resources
are never tasked, and geometry and policy are respected.
- Rationale: MT-01, MT-02, MT-04, MT-06; MOE-03.
- Measures: MOE-01, MOE-03, MOP-06, the allocation row (exact match on the value
function).
- Maturity: I1 partial (facade, last-good-plan, readiness rule) / I2 partial / I3 full
with the allocator, layers, and costs / I4 full.
- Threads: MT-01, MT-02, MT-04, MT-06.
- Provided by: `gungnir-intercept-service` (facade), `gungnir-allocation` (returns
`NotImplemented`), `gungnir-assessment` (rewards).
- Design: `../../design/DN-04-effector-model.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: effector cost and availability data (materiel, policy).

**CAP-3.4 Compute intercept geometry**

- Statement: each assignment shall carry the predicted intercept point and time so
the operator and the fire unit can judge it and the geofence policy can check it.
- Rationale: MT-01, MT-02, MT-04; the policy cannot check a point that does not exist.
- Measures: MOP-25.
- Maturity: I1 none / I2 none / I3 full / I4 full.
- Threads: MT-01, MT-02, MT-04.
- Provided by: `gungnir-intercept-service` (fields present, values pending),
`gungnir-coord`.
- Considerations: effector performance envelopes (materiel).

**CAP-3.5 Offer alternatives, what-if, and rationale**

- Statement: for every recommendation the sector shall be able to show why, offer
ranked alternatives each already policy-checked, and answer "what if" against a
hypothetical snapshot without touching live state.
- Rationale: MT-01, MT-02; automation-bias mitigation; supervisor comparisons.
- Measures: MOP-29 (rationale present on every plan: 1.0), MOP-07.
- Maturity: I1 partial (rationale) / I2 partial / I3 full / I4 full.
- Threads: MT-01, MT-02, MT-06, MT-09.
- Provided by: `gungnir-decision` (rationale implemented; alternatives and what-if
trait surface).
- Considerations: none.

**CAP-3.6 Enforce rules of engagement**

- Statement: no recommendation shall be presented as actionable unless it satisfies
the identification criteria for its class, the weapons control status of its layer,
the engagement authority of the deciding role, and every restriction expressed as a
geofence or rule; escalation and timeout behaviour shall be explicit.
- Rationale: the boundary between decision support and an unsupervised weapon (MT-01
to MT-04, MT-06, MT-10).
- Measures: MOE-02, MOE-05, the policy rows (no plan approved inside a no-go fence;
clean plans still need a human).
- Maturity: I1 partial (geofence and readiness, chain never self-approves) / I2
partial / I3 full with status, authority by role and class, escalation / I4 full.
- Threads: MT-01 to MT-04, MT-06, MT-09, MT-10.
- Provided by: `gungnir-policy` (implemented; status and authority pending),
`gungnir-geo`, `gungnir-security`.
- Design: `../../design/DN-08-policy-configuration.md`, `../../design/DN-09-authority-and-control-status.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: rules of engagement themselves (policy, leadership); training.

**CAP-3.7 Manage the queue under saturation**

- Statement: when recommendations arrive faster than the authority can decide, the
supervisor shall see the queue ordered by priority and time remaining, with
pre-delegated cases handled by policy, and the system shall never silently drop or
reorder a pending decision.
- Rationale: MT-01; MOE-04.
- Measures: MOE-04, MOP-30 (no pending item lost or reordered without a record).
- Maturity: I1 partial (pending list) / I2 partial / I3 full / I4 full.
- Threads: MT-01, MT-02.
- Provided by: `gungnir-command` (queue implemented; ordering and delegation pending),
`gungnir-workflow`.
- Design: `../../design/DN-10-queue-expiry-and-escalation.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: delegation policy (policy).

**CAP-3.8 Recommend fires tasks with deconfliction**

- Statement: for land targets the sector shall propose a fires task with target
location and uncertainty, time window, and deconfliction against friendly positions,
airspace measures, no-fire areas, and interceptor trajectories, for the fires
authority to decide.
- Rationale: MT-06.
- Measures: MOE-08.
- Maturity: I1 none / I2 none / I3 full (D-07, 2026-09-04) / I4 full.
- Threads: MT-06.
- Provided by: `gungnir-intercept-service`, `gungnir-policy`, `gungnir-command`
(domain-neutral; fires plan type pending).
- Design: `../../design/DN-05-fires.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: fires coordination measures (policy, doctrine).

**CAP-3.9 Recommend sensor re-tasking under degradation**

- Statement: when coverage is lost the sector shall propose mode changes and re-tasking
that restore the most valuable coverage, showing coverage before and after.
- Rationale: MT-07.
- Measures: MOE-10.
- Maturity: I1 none / I2 partial / I3 full / I4 full.
- Threads: MT-07, MT-09.
- Provided by: `gungnir-sensor-management`, `gungnir-analytics`, `gungnir-decision`.
- Design: `../../design/DN-13-sensor-retasking.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: none.

## CAP-4 Act (recommend and authorize)

**CAP-4.1 Present recommendations for decision**

- Statement: every recommendation shall be presented to the role that holds authority
with its verdict, rationale, alternatives, estimated cost, and time remaining, with
the accept control never the default.
- Rationale: MT-01 to MT-04, MT-06; automation bias.
- Measures: MOP-07, MOE-04.
- Maturity: I1 partial (plan panel) / I2 partial / I3 full with the approval queue
panel / I4 full.
- Threads: MT-01 to MT-04, MT-06.
- Provided by: `gungnir-ui` (intercept panel; approval controls pending),
`gungnir-workflow`.
- Considerations: display standards and training (training, doctrine).

**CAP-4.2 Record every decision**

- Statement: every accept, override, or reject shall be recorded with the deciding
operator, the plan, the verdict and evidence at the time, and the mission time, and
the record shall be append-only.
- Rationale: MOE-05; audit and after-action review.
- Measures: MOE-05, the command rows.
- Maturity: I1 full (workflow) / I2 full / I3 full wired / I4 full.
- Threads: MT-01 to MT-04, MT-06, MT-10.
- Provided by: `gungnir-command` (implemented; wiring pending), `gungnir-security`
(audit).
- Considerations: retention policy (policy).

**CAP-4.3 Never execute without a recorded human decision**

- Statement: no path in the system shall cause an effector to act without a recorded
human decision; self-defense engagements taken by fire units are recorded after the
fact; agent and model outputs have no authority.
- Rationale: the recommendation-only rule (`../../gungnir-capabilities.md` §5.4).
- Measures: MOE-05, MOP-31 (zero execution paths without a decision record, by
review and by test).
- Maturity: I1 full by design / I2 full / I3 full / I4 full.
- Threads: all.
- Provided by: `gungnir-policy`, `gungnir-command`, plan 08 and plan 09 rules.
- Considerations: doctrine and rules of engagement (policy, leadership).

**CAP-4.4 Hand off to effector systems with provenance**

- Statement: a decided assignment or fires task shall be handed to the effector or
fires system through the versioned interface with the decision record and the
provenance of the track it targets.
- Rationale: MT-01, MT-02, MT-04, MT-06.
- Measures: MOP-32 (handoff latency from decision: under 500 ms).
- Maturity: I1 none / I2 none / I3 partial (endpoint defined) / I4 full with the
transport.
- Threads: MT-01, MT-02, MT-04, MT-06.
- Provided by: `gungnir-api` (handoff endpoint pending), `gungnir-remote`.
- Design: `../../design/DN-07-handoff.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: effector interface agreements (policy, materiel).

**CAP-4.5 Warn assets and authorities**

- Statement: when a threat is predicted to reach an asset, the sector shall warn the
asset, its units, and the civil authorities it is obliged to warn, with the lead
time the plan requires, and record the warning.
- Rationale: MT-01, MT-02, MT-04; warning is the only response to some threats.
- Measures: MOE-01 (warning lead time), MOP-33 (warning issued within 5 s of the
prediction crossing the threshold).
- Maturity: I1 none / I2 partial (alerts) / I3 full / I4 full with external channels.
- Threads: MT-01, MT-02, MT-04.
- Provided by: `gungnir-workflow` (alert lifecycle), `gungnir-observability`,
`gungnir-api`.
- Design: `../../design/DN-03-warning.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: warning channels and agreements (facilities, policy).

**CAP-4.6 Track engagements and assess effects**

- Statement: after handoff the sector shall track the engaged threat, record whether
it was destroyed, missed, or continued, supersede the plan, and feed the outcome to
assessment and review.
- Rationale: MT-01, MT-02, MT-04, MT-06.
- Measures: MOE-01, MOP-34 (outcome recorded within two update cycles after
observation).
- Maturity: I1 none / I2 partial (events) / I3 full / I4 full.
- Threads: MT-01, MT-02, MT-04, MT-06.
- Provided by: `gungnir-model` (events), `gungnir-intercept-service`.
- Design: `../../design/DN-06-engagement-and-effect.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: effector reporting (materiel).

**CAP-4.7 Assist roles without authority**

- Statement: every role shall be able to ask questions about the live or recorded
picture, get explanations of scores, plans, and alerts, and receive drafts of
reports, handovers, and configuration, each answer carrying its provenance and the
assistant holding no authority to act.
- Rationale: workload under saturation (MT-01), handover and review (MT-09).
- Measures: MOP-35 (factual accuracy on the evaluation set: thresholds set by plan 08 with its evaluation set),
MOP-31.
- Maturity: I1 none / I2 none / I3 partial (desktop assistant) / I4 full in all
profiles.
- Threads: MT-01, MT-07, MT-08, MT-09.
- Provided by: plan 08 `gungnir-agent`.
- Considerations: data-egress policy per profile (policy).

## CAP-5 Sustain

**CAP-5.1 Journal every event durably**

- Statement: every envelope the bus carries shall be on disk in the session journal
within the durability budget, and the journal shall be the system of record.
- Rationale: replay, review, reconciliation, audit (MT-09, MT-10).
- Measures: MOP-10 (under 100 ms), the store rows.
- Maturity: I1 full / I2 full / I3 full / I4 full with fsync policy.
- Threads: all.
- Provided by: `gungnir-store` (implemented; wired).
- Considerations: storage sizing and retention (materiel, policy).

**CAP-5.2 Replay and rehearse**

- Statement: any recorded session or test-track scenario shall replay deterministically
through the pipeline on the same screens, under a chosen plan, for review and
rehearsal.
- Rationale: MT-09.
- Measures: MOP-15, MOE-12.
- Maturity: I1 partial (replay of journals) / I2 partial / I3 full with scenario replay
through the live pipeline / I4 full.
- Threads: MT-09.
- Provided by: `gungnir-replay` (implemented), `gungnir-mission`, plan 07.
- Considerations: none.

**CAP-5.3 Produce reports and measures from the journal**

- Statement: after-action reports, measures of effectiveness and performance, and
exports shall be computed from the journal, never from live state, with every figure
traceable to it.
- Rationale: MT-09; `measures.md` §3.
- Measures: the reporting rows; MOE-12.
- Maturity: I1 partial (counts and export) / I2 partial / I3 full with the measures
catalogue / I4 full.
- Threads: MT-09.
- Provided by: `gungnir-reporting` (implemented), `gungnir-metrics`.
- Design: `../../design/DN-20-after-action-review.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: report formats required by higher command (policy).

**CAP-5.4 Operate disconnected and reconcile**

- Statement: a desktop that loses its node shall keep operating on embedded services
with the state shown, queue what it records, forward it on reconnection, and
reconcile the journals with every conflict reported and resolved under the
arbitration rule.
- Rationale: MT-10.
- Measures: MOP-11, MOP-12, MOP-13, MOE-11.
- Maturity: I1 partial (fallback at startup, queues, reconciliation report) / I2
partial / I3 partial / I4 full with mid-session failover and the transport.
- Threads: MT-10.
- Provided by: `gungnir-remote`, `gungnir-resilience`, `gungnir-collab` (implemented;
transport pending).
- Considerations: delegation of authority for the disconnected case (policy).

**CAP-5.5 Report health honestly and run the alert lifecycle**

- Statement: every subsystem's health shall be reported from what it knows, never
inferred; related alerts shall be correlated into incidents; every incident shall
move through acknowledgement, escalation, and closure with a record.
- Rationale: MT-07; MOE-06.
- Measures: MOP-08, MOP-18, MOE-10.
- Maturity: I1 full (health, correlation, lifecycle) / I2 full / I3 full / I4 full with
the node health endpoint.
- Threads: MT-07, and every live thread.
- Provided by: `gungnir-observability`, `gungnir-workflow` (implemented).
- Considerations: none.

**CAP-5.6 Manage configuration baselines and mission plans**

- Statement: sensors, resources, the asset list, policy, and backend settings shall be
managed as validated, versioned baselines; a plan shall be validated before it is
applied and every apply audited.
- Rationale: MT-09.
- Measures: the config rows; MOP-36 (invalid baseline never applied: 1.0).
- Maturity: I1 full for the current schema / I2 full / I3 full with policy and asset
list / I4 full.
- Threads: MT-09, MT-10.
- Provided by: `gungnir-config` (implemented), `gungnir-mission` (trait surface).
- Design: `../../design/DN-01-defended-assets.md`, `../../design/DN-08-policy-configuration.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: configuration authority (policy).

**CAP-5.7 Govern algorithm and model baselines**

- Statement: which filter, association, and learned-model configuration is in force
shall be a validated, promoted baseline per mission profile, with rollback.
- Rationale: MT-09; plan 09.
- Measures: the modelops rows.
- Maturity: I1 full (registry) / I2 full / I3 full wired to the tracking configuration
/ I4 full with learned models.
- Threads: MT-09.
- Provided by: `gungnir-modelops` (implemented; wiring pending).
- Considerations: validation authority (policy).

**CAP-5.8 Support the battle rhythm**

- Statement: shift handover summaries, reporting cycles, and maintenance windows shall
be supported from the journal and configuration, so nothing depends on memory.
- Rationale: MT-09.
- Measures: MOE-12, MOE-13.
- Maturity: I1 none / I2 partial / I3 partial / I4 full.
- Threads: MT-09.
- Provided by: `gungnir-reporting`, plan 08 (handover drafts).
- Design: `../../design/DN-21-battle-rhythm.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: battle-rhythm doctrine (doctrine).

**CAP-5.9 Provide role-based workspaces and workflow**

- Statement: each role shall see the panels its tasks need and no more, with the
alert and approval workflows designed for time pressure and high stakes.
- Rationale: all threads; plan 06.
- Measures: MOP-37 (usability measures from plan 06: time to acknowledge, decision
latency, error rate).
- Maturity: I1 partial (layouts per role defined) / I2 partial / I3 full / I4 full.
- Threads: all.
- Provided by: `gungnir-workflow`, `gungnir-ui`, plan 06.
- Considerations: training and display standards (training, doctrine).

**CAP-5.10 Meet performance budgets per profile**

- Statement: each deployment profile shall meet its end-to-end latency, throughput,
memory, and recovery budgets under mission-scale scenarios.
- Rationale: `../../performance-budgets.md`.
- Measures: MOP-01, MOP-02, MOP-16, MOP-17, and the rest of the budget table.
- Maturity: I1 none (harnesses absent) / I2 partial / I3 partial / I4 full.
- Threads: all.
- Provided by: every crate; the benches and harnesses.
- Considerations: hardware per profile (materiel).

## CAP-6 Secure

**CAP-6.1 Authenticate operators and callers**

- Statement: every operator and every API caller shall be authenticated before any
action, by a mechanism appropriate to the profile.
- Rationale: MT-08, MT-10; `ARCHITECTURE.md` §8.5.
- Measures: the security rows.
- Maturity: I1 none / I2 partial / I3 full on the desktop / I4 full for API callers.
- Threads: all.
- Provided by: `gungnir-security` (trait only).
- Considerations: credential mechanism decision (policy).

**CAP-6.2 Authorize by role, class, and layer**

- Statement: what a role may do shall be enforced per action, refined by threat class
and effector layer for engagement decisions, with unknown operators permitted
nothing.
- Rationale: the authority matrix in `../roles-and-stakeholders.md` §4.
- Measures: the security rows; MOP-38 (matrix enforced under test: 1.0).
- Maturity: I1 full for coarse actions / I2 full / I3 full with class and layer / I4
full.
- Threads: all decision threads.
- Provided by: `gungnir-security` (implemented coarse matrix), `gungnir-policy`.
- Considerations: authority policy (policy, leadership).

**CAP-6.3 Audit everything that matters**

- Statement: every decision, configuration change, model promotion, sensor tasking,
and assistant exchange shall be in an append-only audit log with the actor and
mission time.
- Rationale: accreditation and review.
- Measures: MOP-39 (one audit entry per gated action: 1.0).
- Maturity: I1 partial (log exists; not wired) / I2 partial / I3 full / I4 full.
- Threads: all.
- Provided by: `gungnir-security` (audit log implemented; wiring pending).
- Considerations: retention and access to the audit log (policy).

**CAP-6.4 Protect data in transit and at rest**

- Statement: connected profiles shall encrypt data in transit; cloud nodes shall
encrypt the journal at rest with keys held off the host.
- Rationale: `ARCHITECTURE.md` §8.5.
- Measures: MOP-40 (compliance assessment: zero open high or critical findings at release).
- Maturity: I1 none / I2 none / I3 partial / I4 full.
- Threads: MT-10, MT-08.
- Provided by: `gungnir-api` transport, `gungnir-store`, deployment.
- Design: `../../design/DN-22-key-management.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: key management (materiel, policy).

**CAP-6.5 Assure the supply chain**

- Statement: releases shall be built from audited dependencies, carry a software bill
of materials, be signed, and be reproducible; model artefacts likewise.
- Rationale: `../../release-governance.md`.
- Measures: MOP-41 (release gates pass: 1.0 per release).
- Maturity: I1 full (policy and workflow) / I2 full / I3 full / I4 full with signed
models.
- Threads: MT-09.
- Provided by: `deny.toml`, `release.yml`, plan 09 model signing.
- Considerations: registry and signing infrastructure (facilities).

**CAP-6.6 Mark and enforce releasability**

- Statement: pictures and products exchanged with peers shall carry releasability
marking enforced at the interface.
- Rationale: MT-08, coalition exchange.
- Measures: MOP-42 (no unmarked product released: 1.0).
- Maturity: I1 none / I2 none / I3 partial / I4 full.
- Threads: MT-05, MT-08.
- Provided by: `gungnir-security`, `gungnir-api` (pending).
- Design: `../../design/DN-17-releasability.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: marking policy (policy).

**CAP-6.7 Treat external input as untrusted**

- Statement: every message from a sensor, peer, or person outside the system,
including free text shown to the assistant, shall be treated as data and never as an
instruction, and validated or quoted accordingly.
- Rationale: MT-07; plan 08 prompt-injection defenses.
- Measures: MOP-20, MOP-35 (injection-resistance cases).
- Maturity: I1 full for structured input / I2 full / I3 full with the assistant / I4
full.
- Threads: all.
- Provided by: `gungnir-ingest`, plan 08.
- Considerations: none.

## CAP-7 Integrate

**CAP-7.1 Expose a versioned interface**

- Statement: desktops, peers, and analytics shall integrate through one versioned
interface offering snapshot, event stream, detection submission, and plan decision,
with authorization per call and compatibility rules.
- Rationale: `../../gungnir-api-v1.md`; MT-01, MT-02, MT-10.
- Measures: the API rows; MOP-02.
- Maturity: I1 partial (contract) / I2 partial / I3 partial / I4 full with the
transport.
- Threads: MT-01, MT-02, MT-05, MT-08, MT-10.
- Provided by: `gungnir-api` (types; transport pending).
- Considerations: interface control agreements (policy).

**CAP-7.2 Speak the domain's interoperability standards**

- Statement: the sector shall exchange tracks and observations in ASTERIX, STANAG
4676, Arrow, and JSON through a governed schema catalogue with version negotiation.
- Rationale: MT-05, MT-08; the interop schema row.
- Measures: the interop rows; the cross-layer conformance row.
- Maturity: I1 partial (catalogue, Arrow) / I2 partial (first codec) / I3 full / I4
full.
- Threads: MT-01, MT-05, MT-06, MT-08.
- Provided by: `gungnir-interop` (catalogue and Arrow implemented; codecs pending).
- Considerations: standard licences and test corpora (materiel).

**CAP-7.3 Run as desktop, on-prem node, or cloud node**

- Statement: the same crates shall run as a disconnected desktop, an on-prem service
node, or a cloud service node, selected by configuration, with the desktop switching
backend without UI change.
- Rationale: `ARCHITECTURE.md` §8; MT-10.
- Measures: the app backend-switching row; MOP-11.
- Maturity: I1 full for the desktop, node runnable / I2 partial / I3 partial / I4 full
connectable.
- Threads: MT-10.
- Provided by: `gungnir-app`, `gungnir-node`, `gungnir-remote`, `gungnir-config`.
- Considerations: hosting per profile (facilities).

**CAP-7.4 Exchange with peers and coalition**

- Statement: the sector shall exchange pictures, warnings, and products with
neighbouring sectors, higher command, and coalition partners within agreed latency
and with releasability preserved.
- Rationale: MT-01, MT-02, MT-05, MT-08.
- Measures: MOP-22, MOP-42, MOE-13.
- Maturity: I1 none / I2 partial / I3 partial / I4 full.
- Threads: MT-01, MT-02, MT-05, MT-08.
- Provided by: `gungnir-api`, `gungnir-interop`, `gungnir-security`.
- Design: `../../design/DN-18-coalition-exchange.md` (plan 11, first draft 2026-09-05; awaiting review).
- Considerations: agreements with each peer (policy, organization).

## Open items

- Every target value and maturity level is proposed; the owner confirms or edits.
- Measures MOP-19 to MOP-42 and MOE-13 are new in this document and defined in
  `measures-catalogue.md`; their targets are drafts.
- CAP-3.8 (fires) and CAP-6.6 (releasability) were decided on 2026-09-04: fires in the
  first release (D-07), releasability modelled in I3 and enforced in I4 (D-06); see
  `../gap-analysis/decisions-needed.md`.

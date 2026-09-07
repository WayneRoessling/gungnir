# Machine-learning use cases

Status: first draft, 2026-09-04. Five use cases. **ML-01 and ML-04 are specified in
depth and are the first two to build** (owner decision, 2026-09-04); ML-02, ML-03, and
ML-05 are outlines that follow the same template.

The rule every use case obeys: a model produces **evidence or a score**, never an
action. The consumer that already exists decides what to do with it, and a human
decides what to do with that.

## Template

Each use case states the decision it informs, its inputs and outputs, the consumer
crate, the accuracy target, the latency budget, the failure behaviour, and how a human
sees it. A use case is not buildable until all seven are filled in.

---

## ML-01 Classification from kinematics and evidence

**The decision it informs.** Whether a track is hostile, friendly, neutral, or unknown
(CAP-2.6), which gates every engagement recommendation and carries the fratricide risk
(MOE-02).

**Inputs.** From `TrackView` over a window: speed, altitude, climb rate, heading
stability, turn rate, and their variability; time since initiation; `Quality`
association confidence; the count and mix of contributing sensors from `Provenance`;
whether a cooperative identity is present (GAP-010). No position relative to a defended
asset, deliberately: a track is not hostile because of where it is going, and mixing
intent into class produces a model that declares hostility from geometry.

**Output.** Class probabilities over the four `Classification` values, emitted as one
`gungnir_identification::IdentificationEvidence` with
`source = "ml:<model-name>@<version>"`, `suggested` = the highest-probability class, and
`confidence` = its probability.

**Consumer.** `gungnir-identification`. The existing `EvidenceFusionEngine` fuses it
with cooperative, kinematic, and operator evidence under its decision margin. The model
is one voice among several and never the only one: a hostile declaration on model
evidence alone is prevented by the per-class policy thresholds of GAP-018.

**Accuracy target (proposed).** On the test-track holdout: macro-averaged F1 at least
0.85 across the four classes; **recall on friendly and civil classes at least 0.98**,
because a friendly misclassified as hostile is the failure that matters. Calibration:
expected calibration error at most 0.05, because the confidence is consumed as a weight
and an overconfident model corrupts the fusion.

**Latency budget.** p99 under 5 ms per track on the node's CPU execution provider, for
up to 200 tracks per tick within the per-frame budget of `../performance-budgets.md`.
Inference is batched per tick, not per track.

**Failure behaviour.** If the model is missing, fails to load, or the feature window is
incomplete, `gungnir-ml` returns no evidence for that track and reports the absence in
health. The identification engine proceeds on its other evidence. It never emits a
default class, and it never emits `Unknown` as if it were a finding.

**How a human sees it.** In the evidence card (`../ux/wireframes/WF-04-track-detail-evidence.puml`)
as one row: source `ml:classifier@1.2.0`, kind "model", weight, time. The operator can
see the model's contribution and what the classification would be without it.

**Domain of validity.** Trained on the classes in `../test-tracks/classes.yaml`. A track
whose kinematics fall outside every class envelope is out of domain; the model reports
low confidence and the drift monitor counts it.

---

## ML-04 Sensor-feed anomaly detection

**The decision it informs.** Whether a source or a track is behaving inconsistently with
what it claims (CAP-2.9): a vessel with its cooperative reporting switched off, a
spoofed position, a loitering track, or a sensor emitting implausible data (MT-05,
MT-07).

**Inputs.** Per source, over a rolling window: detection rate against its configured
update period, measurement residual statistics, the fraction of detections that fail to
associate, clock-skew estimate (GAP-008), and the gap since last receipt. Per track:
speed and heading consistency with the declared class, cooperative-identity presence and
its consistency with the kinematic position, and dwell inside a sensitive area.

**Output.** An anomaly score in the range 0 to 1 per source and per track, with the
contributing feature named.

**Consumer.** `gungnir-observability`, which raises a `gungnir_observability::Alert`
through the alert lifecycle when the score crosses the configured threshold.
`gungnir-ingest` is **not** a consumer: an anomaly score never quarantines anything.
Quarantine stays with the gateway's deterministic validation rules, because a learned
model must not be able to silence a sensor.

**Order of precedence.** D-13 already decided that rule-based detectors live in
`gungnir-analytics` and the binaries raise alerts from them. The model is an **addition**
to those rules, not a replacement: when both fire on the same source, the incident
correlates into one alert and names both.

**Accuracy target (proposed).** On the test-track scenarios that carry planted anomalies
(TT-05 has an AIS-off loiterer and a spoofed position; TT-07 has clock skew and a lost
radar): detection rate at least 0.9 on planted anomalies, and **at most 1 false alert
per source per 24 hours**, because an anomaly detector that cries wolf is worse than
none under the alert-storm failure mode the operator task analysis identifies.

**Latency budget.** Under 120 s from anomaly onset to alert, which is MOP-26, already
confirmed by the owner. This is generous by design: the model runs on a rolling window
and does not need to be fast.

**Failure behaviour.** If the model is absent or fails, the rule-based detectors continue
alone and health reports that the learned detector is not running. No anomaly is
reported as absent when the detector is not running; the operator sees "learned
detection off", not silence.

**How a human sees it.** As an incident in the alerts panel
(`../ux/wireframes/WF-08-alerts-incidents.puml`) with the contributing feature in the
summary, the raw alerts inside, and the source labelled as the model. Acknowledging,
escalating, and closing follow the normal lifecycle.

**Domain of validity.** Sensor types in `../test-tracks/sensors.yaml` at the parameters
the scenarios use. A sensor whose configuration differs materially is out of domain.

---

## ML-02 Learned threat scoring (outline)

Informs prioritization (CAP-3.2). Inputs: class, predicted asset and time to impact,
asset priority, kinematics. Output: a score with contributing factors, as an alternative
`gungnir_assessment::ThreatAssessor`. Consumer: `gungnir-assessment`. Target: agreement
with the deterministic baseline within a stated band, and monotonicity in time to impact
and asset priority preserved (MOP-28 applies to the learned scorer too, and a model that
violates monotonicity fails validation). Failure: the deterministic assessor runs.
Presentation: beside the baseline score, never instead of it, until the owner promotes
it. **Not scheduled**: scoring is the input to engagement recommendations, and a learned
scorer needs operational data rather than synthetic tracks before it is trustworthy.

## ML-03 Track-quality and staleness prediction (outline)

Informs the operator's trust in a track (CAP-2.2). Inputs: update history, residuals,
sensor coverage. Output: probability the track goes stale within the next N seconds.
Consumer: the tracking-service projection. Presentation: a visual state that precedes the
deterministic stale flag; the deterministic flag remains the one that blocks allocation.
Failure: no prediction, the deterministic flag alone.

## ML-05 Gating assistance (outline)

Advisory input to `gungnir-association` only. Output: a suggested gate scale. **Not shown
to any operator**, logged only, and the associator's verified gate makes the decision.
This use case exists to be measured, not to be trusted: it is the one place a model
touches the tracking core, and the verification rows for association apply unchanged
with the model on and off.

## What is deliberately not a use case

- Anything that produces a plan, a decision, a quarantine, a sensor mode, or a
  configuration change. Those are the human decision boundary (CAP-4.3) and no model
  reaches them.
- Replacing the Kalman, association, or allocation mathematics. Those stay classical and
  oracle-verified (plan 09 scope).
- Identity assignment across sessions. `gungnir-identity` correlation is deterministic
  and inspectable; a learned matcher would make entity lineage unexplainable.

## Traceability

Capabilities CAP-2.6, CAP-2.9, CAP-3.2, CAP-2.2; gaps GAP-018, GAP-021; measures
MOE-02, MOP-05, MOP-26, MOP-28; decision D-13; data `../test-tracks/`.

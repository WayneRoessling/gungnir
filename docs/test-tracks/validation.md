# Validation

Status: first draft, 2026-09-04. The checks every generated set must pass before it
is committed or used, who runs them, and what a failure means. The automated checks
are `tools/validate_tracks.py`; the Rust replay test is
`gungnir-ingest/tests/test_track_samples.rs`; subject-matter review is recorded in
`vehicle-catalogue.md`.

## 1. Automated checks per set (`validate_tracks.py`)

| Check | Rule | Source of the rule |
|---|---|---|
| Files present | metadata, truth, detections, sensors, events | `data-format.md` |
| Versions current | the set's catalogue, classes, sensors, and scenarios versions equal the YAML versions | `data-format.md` §9 |
| Truth speed within the class envelope | speed at most 1.15 times the class maximum plus 0.5 m/s | `classes.yaml` envelopes |
| Truth altitude within the class envelope | altitude inside the class band (ballistic classes allowed their apogee) | `classes.yaml` |
| No teleports | distance between consecutive records at most 1.3 times the class maximum speed times the interval | physical plausibility |
| Time monotonic per entity | strictly increasing `t` | `data-format.md` §4 |
| Alive flag never revives | once `alive` is false it stays false | `data-format.md` §4 |
| Counts match metadata | truth records, entities, detections | `data-format.md` §7 |
| Detections parse | every line is a `DetectionView` with finite numbers and the provenance fields | `gungnir_ingest::gateway::decode_json_line` |
| Gateway rules | magnitude under 10,000 km; source time at most 1 s ahead of receipt; receipt at most 5 s after source | `gungnir_ingest::gateway::validate_detection` |
| Known sensors | every detection's sensor is in `sensors.json` | `data-format.md` §5 |
| Receipt order | the file is in non-decreasing receipt time | `data-format.md` §3 |
| Out-of-order fraction | per sensor, the fraction of source-time inversions is at most four times the model's fraction plus 0.08 | `sensors.yaml` |
| Expected entity count | full sets only: the scenario's expected entity count | `scenarios.yaml` |

A failing check leaves `validation.passed = false` in `metadata.json` and the set
must not be committed.

## 2. Replay through the product (`test_track_samples.rs`)

Every committed sample set is opened by `RecordedFeedAdapter`, fed through
`IngestGateway` with the allow-all authenticator as mission time advances, and must
produce zero `Quarantined` events, one `Accepted` per line, and a healthy gateway.
The test runs in `cargo test --workspace` and therefore in CI (`ci.yml`). It is the
acceptance evidence that the format and the gateway agree.

## 3. Statistical self-checks (from the verification table, once the pipeline exists)

When `gungnir-scenario` generates these sets and the tracking pipeline runs, the §1
rows of `../verification-capability-table.md` apply to the sets as inputs: track
continuity per scenario, false-track rate under 1 per hour in TT-05 clutter
(MOP-04), identity continuity across TT-06's gap (MOE-09), registration bias
recovered within tolerance for TT-06's biased sensor, and replay determinism
(MOP-15). `detections-truth.jsonl` gives the association truth those rows score
against.

## 4. Plausibility review (human-owned)

- A subject-matter reviewer per domain checks the class profiles' phases and ranges
  and the scenario compositions against public reporting; findings go into
  `classes.yaml` and `scenarios.yaml` and the catalogue's validation record.
- Reviewers are the ones named in `../mission/mission-analysis.md` §11; none has
  reviewed yet.

## 5. Size limits for committed sets

Proposed (plan 07 open question): at most 1.5 MB per file and 10 MB for the whole
`testdata/tracks/samples/` folder; the `sample` blocks in `scenarios.yaml` are tuned
to stay under it. As committed on 2026-09-04 the folder is 6.5 MB and the largest
file is 772 KB (`TT-07-sample/truth.jsonl`). Full sets are never committed.

## 6. Record

| Date | Sets | Automated | Replay test | Review |
|---|---|---|---|---|
| 2026-09-04 | TT-01 to TT-10 samples | 18 checks each, all passing (`validation-report.json` per set) | `every_sample_set_replays_without_quarantine` passes | none yet |

Two failures the checks caught on the first run, both fixed in the models rather than
by loosening the check: an electronic-attack clock skew pushed the apparent latency
past the gateway's receipt window, so the generator now counts skew against the
latency cap and skewed data still reaches the tracker, where the skew must be
detected (MOP-09) rather than silently quarantined; and TT-08's civil traffic used a
multirotor and a fighter airframe as stand-ins for a light aircraft and an airliner,
putting truth outside the class envelopes, so both now have their own classes and
sourced platform entries.

## 7. Prediction-error tolerances (MOP-25, confirmed 2026-09-05)

MOP-25 was deferred to this plan by D-16. The truth files make it measurable: the
predicted impact point and time are compared with the entity's actual impact, and the
predicted closest point of approach with the actual one, at a fixed horizon before
the event. **The owner confirmed the per-class values below on 2026-09-05**, with one
caveat recorded at confirmation:

> These tolerances assume the **filter-based** predictor, `PredictorKind::Filter`, which
> waits on GAP-011. The only predictor implemented today is
> `gungnir_assessment::ConstantVelocityPredictor`, which propagates in a straight line.
> Two rows are not expected to be met by it and should not be read as failures of the
> measure when they are missed: `air.ballistic-srbm` at 1,000 m over a 30 s horizon, and
> `air.glide-bomb` at 300 m over 30 s. Both describe curving trajectories that a
> straight-line propagation cannot follow. A miss on either row before GAP-011 lands
> means the predictor is not built, not that the target is wrong.

| Class | Horizon | Impact-point error | Time error |
|---|---|---|---|
| `air.owa-prop`, `air.owa-jet` | 60 s | 300 m | 8 s |
| `air.cruise-subsonic` | 60 s | 400 m | 5 s |
| `air.cruise-supersonic` | 30 s | 1,500 m | 4 s |
| `air.ballistic-srbm` | 30 s | 1,000 m | 3 s |
| `air.glide-bomb` | 30 s | 300 m | 5 s |
| `air.loitering` | 30 s, from the start of the dash | 800 m | 15 s |
| `air.fpv`, `air.small-multirotor` | 20 s | 200 m | 5 s |
| `sea.usv` | 120 s | 200 m (closest point of approach) | 20 s |
| land classes | 120 s | 500 m (route prediction; no impact) | not applicable |

The horizons differ because the classes differ: a ballistic missile is predictable
but fast, a loitering munition is unpredictable until it dashes, and a land vehicle
has no impact to predict. The tolerances are looser than the sensor noise alone would
imply because they must hold through manoeuvres and dropouts.

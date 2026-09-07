# Data pipeline

Status: first draft, 2026-09-04. From journals and test tracks to a versioned training
set, with provenance on every row.

## 1. Sources

| Source | What it gives | Labels | Limitation |
|---|---|---|---|
| Test-track sets (`../test-tracks/`) | Truth and observations for ten scenarios, generated deterministically from a sourced catalogue | **Complete**: `truth.jsonl` carries class, side, and phase per entity per tick; `detections-truth.jsonl` maps every detection line to the entity that caused it, or to nothing for a false alarm | Synthetic. The models learn the generator's assumptions as well as the physics |
| Session journals (`gungnir-store`) | Real envelopes from real or recorded sessions with full provenance | **Partial**: operator identity declarations and decisions are labels; everything else is unlabelled | None exist yet at scale; this is the source that matters and the one the project will not have until increment 2 |
| Scenario generator (`gungnir-scenario`) | The five engineering scenarios once implemented (GAP-016) | Complete | Also synthetic |

The honest position, stated in every model card: **the first models are trained on
synthetic data and their domain of validity is the generator's parameter space.** Mixing
in real journals as they arrive is the plan, not a footnote.

## 2. Dataset schema

Arrow, through `gungnir_interop`'s schema catalogue, so datasets and the wire format
share one definition and a dataset can be read by anything that reads the detection
form.

**ML-01 classification rows**, one per track per window step:

| Column | Type | From |
|---|---|---|
| `dataset_version`, `feature_schema_version` | uint32 | The pipeline |
| `source_set`, `scenario`, `seed` | utf8, utf8, uint64 | The set's `metadata.json` |
| `entity_id`, `track_id` | utf8, uint64 | Truth and the tracker |
| `mission_time` | float64 | `MissionTime` |
| `speed_mps`, `altitude_m`, `climb_mps`, `turn_rate_dps` | float32 | `TrackView` window |
| `speed_var`, `heading_var` | float32 | Window statistics |
| `age_s`, `association_confidence`, `sensor_count`, `sensor_mix` | float32, float32, uint8, utf8 | `Quality`, `Provenance` |
| `cooperative_present` | bool | Cooperative evidence |
| **`label_class`** | utf8 | Truth `class` mapped to `Classification` |
| **`label_side`** | utf8 | Truth `side`, used to check the friendly-recall target |
| `split` | utf8 | train, validation, or test |

**ML-04 anomaly rows**, one per source per window and one per track per window, with the
planted-anomaly flag from the scenario's `events.jsonl` and `expected` block as the
label.

## 3. Splits

Split **by scenario and by entity, never by row.** Rows from one entity are correlated in
time, so a random row split leaks the answer across the boundary and produces a model
that scores well and fails in the field. The split is recorded in the dataset and is
part of its version.

Proposed for the first datasets: TT-01, TT-03, TT-04, TT-06, TT-08 to train; TT-02 and
TT-07 to validate; TT-05, TT-09, TT-10 held out for test and never looked at until the
model is otherwise finished.

## 4. Class balance

The scenarios are built for operational realism, not for balance: TT-05 has 139 civil
vessels and TT-01 has 45 hostile drones. Balance is handled at training time by class
weighting, and the friendly and civil recall target exists precisely because the
majority class in a mixed set is not the class whose errors hurt.

## 5. Provenance and versioning

Every dataset carries the four input versions the test-track sets carry (catalogue,
classes, sensors, scenarios), the generator version, the feature-extractor schema
version, the split definition, and the row counts per class. A dataset is identified by a
content hash. A model card names the dataset hash it was trained on, so any model can be
traced back to the exact rows.

## 6. Retention and data handling

- Synthetic datasets are reproducible from their inputs and seeds, so they are
  regenerated rather than archived.
- **Datasets built from real session journals are operational data.** They inherit the
  journal's handling rules: they stay in the deployment's jurisdiction, they are covered
  by the retention policy in `gungnir-config`, and they are not copied into a training
  repository without the customer's agreement. This is the single most likely place for
  a data-handling mistake and is called out in `security.md`.

## 7. What the pipeline does not do

- No automatic labelling from the tracker's own output. Training a classifier on the
  classifications the system already made is how a model learns to repeat its
  predecessor's mistakes with more confidence.
- No augmentation that invents kinematics outside the class envelopes; the envelopes are
  the domain of validity and inventing beyond them makes the domain a fiction.

## Traceability

`../test-tracks/data-format.md` for the source format; `gungnir-interop` for Arrow;
`training.md` for what consumes this; gap GAP-079.

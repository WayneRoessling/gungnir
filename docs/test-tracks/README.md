# Test-track suite

Deliverables of [plan 07](../plans/07-test-track-suite.md): a catalogue of the air,
sea, and land platforms in use by both belligerents in the Russia-Ukraine war, the
kinematic class profiles derived from them, the sensor models that turn truth into
observations, a scenario library aligned with the mission vignettes, the data
format, a reference generator, and validation. Generated data lives under
[`../../testdata/tracks/`](../../testdata/tracks/README.md).

Status: first draft 2026-09-04. Ten sample sets are generated, validated, and
committed, and they replay through the real ingest gateway in `cargo test`. No
subject-matter reviewer has vetted the vehicle data yet, so every figure carries its
source and confidence mark and none should be treated as authoritative.

Everything is open-source only; [`sourcing-and-legal.md`](sourcing-and-legal.md)
governs every figure.

## The documents

| File | Content |
|---|---|
| [`sourcing-and-legal.md`](sourcing-and-legal.md) | The policy that governs the rest: open sources only, source recording, confidence marks, what is deliberately coarse, the export-control note |
| [`vehicle-catalogue.md`](vehicle-catalogue.md) | 58 platforms in 30 kinematic classes, one table per domain, with per-platform pages under [`platforms/`](platforms/) carrying the sources |
| [`class-profiles/`](class-profiles/README.md) | One file per class: the envelope every platform fits, the phases of a representative mission with a movement model each, randomization, signatures, sensors, threads |
| [`sensor-models.md`](sensor-models.md) | The 13 sensor types: range by signature class, detection probability, update period, noise, latency and out-of-order behaviour, dropouts, false alarms, electronic-attack sensitivity |
| [`scenario-library.md`](scenario-library.md) | Ten scenarios TT-01 to TT-10, one per mission vignette, with composition, events, and expected outcomes |
| [`data-format.md`](data-format.md) | Truth, observation, sensor, event, and metadata formats; the observation form is exactly what `gungnir-ingest` already reads |
| [`generation-method.md`](generation-method.md) | How a set is produced, determinism and seeding, how to add a class or scenario, what the reference generator does not model |
| [`validation.md`](validation.md) | The checks every set must pass, the replay test, the statistical self-checks, the prediction-error tolerances proposed for MOP-25 |

The Markdown above is **rendered** from four YAML sources and must not be edited by
hand: `catalogue.yaml` with `catalogue-{air,sea,land}.yaml`, `classes.yaml`,
`sensors.yaml`, and `scenarios.yaml`. Those are the source of truth.

## Generating and validating

From the workspace root:

```bash
python docs/test-tracks/tools/build_catalogue.py
```

renders the documents and refuses to render a platform without a source or a
confidence mark, a class with no platform, or a phase outside its class envelope.

```bash
python docs/test-tracks/tools/gen_tracks.py
```

regenerates the ten committed sample sets into `testdata/tracks/samples/`. Add
`--full` for full-size sets into `testdata/tracks/full/` (not committed), or name
scenarios (`gen_tracks.py TT-01 TT-04`). Output is deterministic under the seed
recorded in each set's `metadata.json`.

```bash
python docs/test-tracks/tools/validate_tracks.py
```

runs the 18 checks in `validation.md` over every sample set and writes
`validation-report.json` into each.

All three need PyYAML. `tools/gen_tracks.py` is the **reference** generator: it is
the executable specification the `gungnir-scenario` generator (GAP-016) must
reproduce, written for clarity rather than speed.

## Using the sets in tests

- `gungnir-ingest/tests/test_track_samples.rs` opens every committed sample through
  `RecordedFeedAdapter`, feeds it through `IngestGateway` as mission time advances,
  and asserts zero quarantines and one accepted detection per line. It runs in
  `cargo test --workspace` and therefore in CI.
- The truth files give the ground truth the tracking rows score against once the
  pipeline exists: association truth per detection line
  (`detections-truth.jsonl`), identity continuity across gaps (MOE-09), the
  injected sensor bias in TT-06 for the registration row, and the class labels for
  identification and for plan 09's training data.
- Benchmark inputs and the fuzz corpus are seeded from these sets under GAP-076.

## The sets

| Set | Scenario | Entities | Truth records | Detections (false alarms) | Sensors |
|---|---|---|---|---|---|
| TT-01-sample | Night raid with decoys | 12 | 1,408 | 408 (155) | 7 |
| TT-02-sample | Cruise-missile salvo | 10 | 855 | 209 (132) | 7 |
| TT-03-sample | Multirotor and fibre-optic FPV | 2 | 284 | 2,070 (304) | 5 |
| TT-04-sample | USV group at night | 9 | 1,454 | 2,279 (365) | 3 |
| TT-05-sample | Dense surface traffic with anomalies | 17 | 2,179 | 1,761 (380) | 3 |
| TT-06-sample | Battery and convoy with a biased sensor | 8 | 2,348 | 642 (123) | 4 |
| TT-07-sample | Raid under GNSS denial with a radar loss | 10 | 3,773 | 954 (733) | 7 |
| TT-08-sample | Mixed friendly, civil, and hostile air traffic | 11 | 1,638 | 1,739 (199) | 5 |
| TT-09-sample | TT-01 under the sea-weighted laydown | 12 | 1,536 | 751 (560) | 10 |
| TT-10-sample | TT-01 with a mid-raid link loss | 12 | 1,536 | 751 (560) | 10 |

6.5 MB committed in total; the largest single file is 772 KB.

## Open items

- Subject-matter review of each domain table (`vehicle-catalogue.md` validation
  record); no reviewer has looked yet.
- The prediction-error tolerances proposed for MOP-25 (`validation.md` §7) need the
  owner.
- Class priority for deepening the catalogue: one-way attack UAS, cruise missiles,
  small UAS, uncrewed surface vessels, then land classes (plan 07 open question).
- Whether the catalogue is released externally with the product
  (`sourcing-and-legal.md` §6).

# Data format

Status: first draft, 2026-09-04. The formats of a generated test-track set: truth,
observations, and metadata; naming, versioning, and provenance. The observation
format is exactly the recorded-feed format `gungnir-ingest` already reads, so a
generated set replays through the gateway with no conversion.

## 1. A set

One directory per generated set:

```
testdata/tracks/samples/TT-01-raid-sample/
  metadata.json          scenario, seed, generator, catalogue and class versions, counts, provenance
  truth.jsonl            one truth record per entity per truth tick
  detections.jsonl       one DetectionView per line, every sensor, in generation order
  sensors.json           the sensor set used, with each sensor's id, type, position, and parameters
  events.jsonl           scenario events (sensor loss, electronic attack, link loss) with times
```

Sets under `testdata/tracks/samples/` are small (reduced composition or excerpt)
and committed; full-size sets are generated on demand into `testdata/tracks/full/`
(ignored by version control) with the same layout.

Naming: `<scenario id>-<variant>` where variant is `sample`, `full`, or a named
variant from the scenario library (`TT-09-laydown-B`).

## 2. Frame and units

- Positions are ENU metres relative to the scenario origin, which is the sector
  command post at Ostmark (0, 0, 0) in every scenario; `metadata.json` records the
  origin's fictional geodetic position so a future geodetic export is possible.
- Times are mission seconds from the scenario start (`MissionTime`), never wall
  clock; `metadata.json` records the fictional start time of day for the story.
- Speeds are metres per second; angles degrees; altitudes metres above the origin's
  ellipsoid height (sea level in the fictional estuary).

## 3. Observations: `detections.jsonl`

One JSON `DetectionView` per line, the `serde` form of
`gungnir_model::DetectionView`:

```json
{"sensor":3,"source_time":95.0,"receipt_time":95.31,
 "measurement":{"Position":{"enu":[41230.5,-12987.2,212.4],"variance_m2":[400.0,400.0,900.0]}},
 "provenance":{"source_sensor_ids":[3],"calibration_baseline_version":"cb-2026-09","algorithm_version":"tt-gen 0.1.0"}}
```

| Field | Type | Meaning |
|---|---|---|
| `sensor` | integer | `SensorId`, matching `sensors.json` |
| `source_time` | number | mission seconds when the sensor observed the target |
| `receipt_time` | number | mission seconds when the system received it; `source_time` plus the sensor's latency and jitter |
| `measurement` | tagged `Measurement` | **Changed 2026-09-06** (`../design/DN-27-bearing-only-detections.md` §4 and §8): the measurement is a tagged enumeration, because a sensor does not always report a place. A generated set is a position feed, so every line here is `{"Position":{"enu":[e,n,u],"variance_m2":[ee,nn,uu]}}` -- ENU metres with the sensor's measurement noise applied, and the per-axis variance the tracking baseline states. The other two variants, `RangeAzimuthElevation` and `Bearing`, are what an acoustic array, a direction finder or a spotter produces; **no generator writes one yet** and a set that contained one would fail `validate_tracks.py`'s shape check rather than be read as a position |
| `provenance.source_sensor_ids` | `[integer]` | the observing sensor |
| `provenance.calibration_baseline_version` | string or null | the calibration baseline the scenario declares; null for uncalibrated sensors |
| `provenance.algorithm_version` | string | the generator name and version |

Rules the gateway enforces and the generator guarantees: every value finite;
measurement magnitude under 10,000 km; `source_time` at most 1 s ahead of
`receipt_time`. Lines are written in generation order, which is receipt order per
sensor; the recorded adapter releases them by source time, so out-of-order arrival
across sensors is preserved by the latency differences, as the tracking core must
handle (`gungnir-time` late-data policy).

False alarms (clutter) are detections with no truth entity; their provenance is the
same as real detections, because the sensor cannot tell.

## 4. Truth: `truth.jsonl`

One record per entity per truth tick (default 1 s):

```json
{"t":95.0,"entity":"TT01-drone-007","class":"air.owa-prop","platform":"shahed-136",
 "side":"red","phase":"cruise","pos":[41200.0,-13000.0,210.0],"vel":[-46.0,14.5,0.0],"alive":true}
```

| Field | Meaning |
|---|---|
| `entity` | synthetic id, stable for the entity's life; the same entity keeps its id through gaps (identity continuity truth for MOE-09) |
| `class`, `platform` | catalogue ids |
| `side` | `red`, `blue`, `civil` |
| `phase` | the class profile phase in force (`launch`, `cruise`, `terminal`, `loiter`, `stop`, `move`, and so on) |
| `pos`, `vel` | ENU metres and metres per second |
| `alive` | false after the entity is destroyed or lands; records stop after that |

Truth for identity: `class` and `side` are the labels the tracker's identification
is scored against (plan 09 training labels); they are never in the observation
stream.

## 5. Sensors: `sensors.json`

```json
{"sensors":[{"id":1,"type":"radar.long","name":"R1 ridge radar","pos":[8000.0,3000.0,450.0],
 "params":{"update_hz":0.5,"range_m":{"large":180000,"medium":120000,"small":60000,"very-small":30000},
 "sigma_m":{"range":30.0,"cross":80.0,"height":150.0},"latency_s":0.3,"jitter_s":0.1,"dropout":0.05,"fa_per_scan":0.2},
 "calibration":"cb-2026-09"}]}
```

Parameters are the resolved values from `sensors.yaml` after scenario overrides;
`sensor-models.md` defines each.

## 6. Events: `events.jsonl`

```json
{"t":1200.0,"kind":"sensor_lost","sensor":1,"note":"ridge radar struck"}
{"t":0.0,"kind":"ea_skew","sensors":[3,5,9,12],"skew_s":2.0,"until":3600.0}
{"t":1380.0,"kind":"link_lost","node":"KAL","until":2040.0}
```

Kinds: `sensor_lost`, `sensor_restored`, `ea_skew` (clock skew applied to the named
sensors' source times), `ea_dropout` (dropout multiplier), `link_lost`,
`link_restored`, `launch_warning` (a peer report at `t`), `decoy_reveal` (truth
annotation only).

## 7. Metadata: `metadata.json`

```json
{"scenario":"TT-01","variant":"sample","seed":1701,"generator":"tt-gen 0.1.0",
 "catalogue_version":"2026-09-04","classes_version":"2026-09-04","sensors_version":"2026-09-04",
 "duration_s":600,"truth_tick_s":1.0,"origin":{"name":"Sector command post, Ostmark (fictional)","lat":0.0,"lon":0.0,"alt_m":0.0},
 "start_time_of_day":"01:38:00","counts":{"entities":12,"truth_records":7200,"detections":9134,"false_alarms":412,"sensors":5},
 "expected":{"entities_by_class":{"air.owa-prop":10,"air.decoy":2}},
 "provenance":{"policy":"docs/test-tracks/sourcing-and-legal.md","generated":"2026-09-04T00:00:00Z"},
 "validation":{"passed":true,"checks":18,"report":"validation-report.json"}}
```

The `seed` and the three version stamps make the set reproducible: the same
generator, seed, and inputs produce byte-identical files (`generation-method.md`).

## 8. Arrow form

For analytics and bulk exchange, `detections.jsonl` converts to Arrow record
batches with `gungnir_interop::detections_to_record_batch`, whose schema is the
one in `gungnir-interop`; no separate Arrow file is committed.

## 9. Versioning

- The format version is the `SCHEMA_VERSION` of `gungnir-model` for observations,
  which went from 2 to 3 on 2026-09-06 when `measurement` became a tagged
  `Measurement` (`../design/DN-27-bearing-only-detections.md` §8). **The committed sets
  were rewritten in that change**, so a set read by a build holding version 2 is refused
  rather than misread;
  truth, sensors, events, and metadata carry `"format": 1` in `metadata.json` and
  change only additively.
- Generated sets record the versions of the three YAML inputs; a set is stale when
  any of them changes, and `validate_tracks.py` reports it.

## Traceability

- `gungnir-ingest/src/gateway.rs` (`decode_json_line`, `validate_detection`);
  `gungnir-ingest/src/adapters/recorded.rs`; `gungnir-model::DetectionView`;
  `gungnir-interop`; the identity truth for MOE-09 and MOP-25.

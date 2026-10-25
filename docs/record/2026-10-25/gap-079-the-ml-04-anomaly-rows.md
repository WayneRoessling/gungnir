# GAP-079: the ML-04 anomaly rows

GAP-079's action left two things: rows from session journals, which wait on real sessions,
and the ML-04 anomaly rows, which did not have to wait. This item records the second.

## What was built

- **The schema.** `gungnir_interop::dataset::anomaly_rows_schema`, registered in the
  catalogue as `gungnir.ml.anomaly-rows` version 1, beside the ML-01 rows. The rows are one
  per source per window and one per track per window, in one schema, told apart by
  `subject_kind`. Every feature is nullable: a feature that does not apply to the row's
  kind, or has nothing to measure in the window, is null, never zero. A zero would read as
  "seen, and perfect", and "nothing seen" is exactly what a lost sensor looks like.
  `../../ml/data-pipeline.md` §2 now has the column table.
- **The extractor.** `gungnir_ml::anomaly::extract_anomaly_set` turns a committed
  test-track set into those rows, with a split by scenario and a content hash, as the ML-01
  extractor does. A window is ten truth ticks (20 s), and windows do not overlap.
- **The interop conformance suite** gains the entry's in-memory arm. Its wire-coverage
  declaration says why it crosses no wire: it is a file for training.

## How the features were chosen

They are ML-04's inputs (`../../ml/use-cases.md`), as far as a set can supply them:
- **Per source:** detections per scan, the unassociated fraction, the residual, a clock-skew
  estimate, and the gap since the last receipt.
- **Per track:** speed, the residual, and three cooperative features — whether a cooperative
  sensor reported the entity, that report's residual, and the gap since its last report.

**The clock-skew estimate is the one a live node can compute without truth:** mean apparent
latency less the sensor's configured mean. The generator applies electronic-attack skew as
a lagging source time and leaves the receipt time true, so a skewed sensor's apparent
latency rises by its skew. A probe over TT-07 read about 2.3 s for the three sensors
lagging 2 s, and 0.04 to 0.19 s for the rest. The one-sided latency jitter is why an honest
clock reads slightly above zero.

Two inputs are left out rather than approximated: consistency with the declared class's
envelopes, and dwell inside a sensitive area. A set carries neither.

## How the labels were found, and two corrections on the way

`data-pipeline.md` said the label came from `events.jsonl` and the `expected` block. The
`expected` block gives only a count, so the labels come from the set's own records instead.
`environment.json` gives the per-sensor events (`ea_skew`, `ea_dropout`, `sensor_lost`), and
`entities.json` gives the per-entity flags (an AIS spoofing offset, an intermittent ADS-B
transponder).

Running the extractor corrected two things the first draft assumed:

- **TT-05's AIS-off loiterer is in the committed sample.** `entities.json` does not record
  the scenario's `ais: false`; it records the emission that results, `none`, where every
  other fishing vessel in the set emits `ais`. So `cooperative-silent` is derived: an entity
  emits `none` where another of its platform in the set emits `ais` or `adsb`. That is
  exactly when the generator writes `none` (`emission_key`). A platform no entity in the set
  shows emitting cannot be judged, and labels nothing.
- **TT-05's spoofed coaster is never detected in the sample.** The northern shipping lane is
  outside every sensor's coverage there. Its rows carry the label and no detection at all,
  because a label is the planted truth whether or not a window could show it. A consumer
  evaluating a detector filters on `detections`.

Across all eleven committed sets the labels land exactly on the planted anomalies:
- **TT-05:** both of the anomalies the scenario expects.
- **TT-07:** clock skew on sensors 3, 5 and 9, dropout on 1 and 2, and sensor 1 lost from
  1200 s.
- **TT-08:** the intermittent transponder.
- **Every other set:** nothing.

## How it was tested

`gungnir-ml/tests/anomaly.rs`:
- **Planted labels.** Every set labels exactly its planted anomalies.
- **Clock skew.** The estimate reads within 0.5 s of TT-07's planted 2 s on the skewed
  sensors, and within 0.5 s of zero on the others.
- **Lost sensor.** Its windows after the loss have no detections, null features where a
  detection is needed, and a growing receipt gap.
- **TT-05.** The silent loiterer is seen by radar and never reported cooperatively, while
  an AIS vessel in the same coverage reports within 100 m. The spoofed coaster carries its
  label and no detection.
- **Nulls.** A feature that does not apply to a row's kind is null.
- **Form and hash.** The rows match the catalogue schema, the split is the scenario's, and
  the hash is stable.

`cargo test -p gungnir-ml -p gungnir-interop` passes, and clippy reports nothing.

## What it does not do

- **No model is trained.** That is GAP-080.
- **No rows come from session journals.** They still wait on real sessions, which is the
  other half of GAP-079.
- **The rows inherit a synthetic dataset's limit.** The set's truth stands in for the
  tracker, as it does for the ML-01 rows, so a residual is the one a perfect tracker would
  see. A model card trained on these rows must say so.

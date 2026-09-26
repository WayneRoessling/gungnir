# GAP-105 re-observation for a laydown built

GAP-105 is built as `../../design/DN-32-re-observation-for-a-laydown.md` designs it, under the
owner's delegation of 2026-09-25 ("run in autonomous mode, select decisions that are blocking
based upon the best production solution that meets the users best experience, no short
cuts"). A laydown rehearsal on PN-16 now re-observes a committed test-track recording with
the selected laydown's own sensors, where it places them, each with the detection model the
deployment names for it, and runs what they would have detected through a throwaway
pipeline. This item records what was built, what the build found, and the three decisions it
took (D-72, D-73, D-74). DN-32 §12 is the design-side account of the same findings.

## What was built, in the order it was committed

1. **`gungnir-sensor-sim`**, a new tracking-core crate: the observation block of
   `gungnir-scenario`'s `tracks.rs`, moved statement for statement with `Num` and
   `PythonRandom`, as `observe` and `false_alarms`; and `reobserve`, the scan loop over a
   recording's truth. The generator calls the moved model through edge (z), and
   `gungnir-scenario/tests/reference_parity.rs` passes **unchanged**: all ten sample sets are
   still reproduced byte for byte, which is what makes the model a rehearsal runs the
   verified one. Every observation carries a `SimulationMark`, set only inside the crate.
2. **The three sidecars, from both generators.** `docs/test-tracks/tools/gen_tracks.py` and
   `gungnir-scenario` write `entities.json` and `environment.json` into every sample set and
   the JSON export of `sensors.yaml` at `testdata/tracks/sensor-models.json`, all with sorted
   keys, so `gungnir-scenario/tests/sidecar_parity.rs` holds them byte for byte rather than as
   equal values. The four data files of every set are unchanged. `validate_tracks.py` gained
   four checks (22 per set), all passing.
3. **Verification of the model against the recordings**:
   `gungnir-sensor-sim/tests/statistical_match.rs` re-observes all ten sets with their own
   sensors at their own positions over twelve fixed seeds -- per-sensor detection counts
   within 2σ of the recording's (worst 1.39σ, TT-06's ground radar), clutter at the configured
   rate (0.34σ), residual variance per axis within 2σ of the model's noise (worst 0.95σ,
   cross-range) -- and `tests/geometry.rs` is the property test that moving a sensor stops,
   then restarts, one target's detections and changes nothing else.
4. **Containment in the model and the gateway.** `Provenance` gained `rehearsal:
   Option<RehearsalOrigin>`, defaulted and left out when `None`, so no existing record
   changes. A live `IngestGateway` refuses a marked detection, counts it as a quarantine
   under `IngestStats::rehearsal_refused`, and publishes a named reason; only
   `IngestGateway::for_rehearsal` admits one (`gungnir-ingest/tests/rehearsal_containment.rs`).
   **The gateway change is human-owned; see `../../signatures.md`.** The Arrow exchange
   refuses a marked detection rather than strip the mark it has no column for.
5. **The rehearsal and PN-16.** `SensorConfig` gained `detection_model`;
   `gungnir-app/src/laydown_rehearsal.rs` resolves each placed sensor's model from the
   catalogue export, refusing by name one that names none or an unknown one, re-observes the
   recording, and feeds the marked detections through the throwaway desktop's rehearsal
   gateway. Edge (aa) carries it; `sensor_sim_misuse` in `dependency_graph.rs` and a source
   test in `architecture_compliance.rs` enforce mechanisms 4 and 5, each checked by adding
   the forbidden thing and reading its name back. PN-16 labels every result "Re-observed
   from a recording", names the recording and each sensor's model, reports per-sensor
   detections, and its options table reads the run: a laydown's rehearsal against the
   current laydown's, over the same recording, with the sensors the difference came from.
   `round-1.json` names `radar.short` for both radars.

## What the build found

**DN-32 §5.1's premise did not hold (D-72).** Both generators step each entity once per
truth tick and run every scan that fell due since the previous tick against the state just
stepped to, so every recorded detection was made of the target as the record of the first
tick at or after its scan has it. Re-observing by that rule is the recording's own rule, not
interpolation. §5.1's finer tick would not have put truth at scan times -- each sensor's
first scan is at a random phase -- and would have rewritten all ten committed sets, which
gated rows measure. The sets keep their 2 s tick; a truth record off the grid is refused.

**The Rust port of the generator resolved one emission class differently from the
reference.** `sensors.yaml`'s `emission_map` lists "control and video link" under both
`datalink` and `control`; the reference takes the first class in the file's order, the port
held the map sorted and took `control`. No committed detection showed it, because no sample
set has a sensor reading that class of a platform that emits it; `entities.json`, which
writes every entity's emission class whether a sensor reads it or not, found it on its first
comparison. The map keeps the file's order now.

**Seeding a rehearsal by the laydown, as DN-32 §7 said, would have defeated the comparison
§10 asks for (D-74).** Two laydowns would draw differently everywhere, and a sensor they
place identically would detect differently under each. Streams are keyed by the recording's
seed and by sensor and target instead. With round 1's own baseline, `current` and `c` give
S1 634 detections under both and S2 1 241 and 1 260, and `b`, which moves a battery,
changes nothing.

**A recording's sensor-specific events cannot be transferred to a deployment's sensors
(D-73)**, for the reason §5.4 gives against matching sensors by identifier. They are
counted and not applied; the sea state is applied to every sensor.

**The marker could not be `gungnir-model`'s type inside `gungnir-sensor-sim`**, which sits
beneath `gungnir-scenario` in the core and may not reach up into the foundation. The crate
carries its own mark and the desktop converts it, totally and checked.

**A sensor's azimuth sector arrived on `main` while this was built** (GAP-118, D-84:
`SensorConfig` and `SensorPlacement` gained `azimuth_sector`). It is honoured: a rehearsal
re-observes with the placement's re-aim, else the declared sector, turned into the
recording's frame, as a gate on top of the detection model's own field of regard, before
any draw; clutter outside the sector is dropped; and a re-aim counts as a move in a
comparison. DN-32 §12 has the reasoning. This applies the owner's D-84 rather than taking
a decision of its own.

**The recording's own clutter is not a baseline to compare a re-observation's with.** TT-08's
sensor 13 recorded 21 false alarms against the 36 its model expects (2.5σ low). The
statistical row compares detections of a recorded target with the recording, and false
alarms with the configured rate, as §1's self-check does.

**No committed recording reaches round 1's radars (GAP-147).** Round 1's harbour and plant
sit at the recordings' origin under §5.5's frame, and no sample set brings a target within
25 km of it before its excerpt ends, so round 1's short-range radars re-observe nothing of
any of them. §10's round-1 row is tested against a recording the test writes -- three drones
down round 1's own declared approach -- and a committed round-1 recording is filed as
GAP-147, being scenario content of the kind the owner decided for round 1 in D-28. PN-16
says so when a rehearsal re-observes nothing, rather than leaving zeros to be read as a
fault. **Laydown `c` is kept**: it still carries the coverage comparison it was added for,
and it is the case the round-1 row is written against.

## Verification rows

DN-32 §10's six rows are in `../../verification-capability-table.md` §2 as **Draft** rows,
each naming the test that implements it. None is a gate, and none was agreed: §10 was not
part of the owner's walk of DN-32 on 2026-09-17.

## Not done here

- **GAP-107** (a plan gated on having been rehearsed) is unchanged in substance: it needs
  its own adoption design. What changed under it is that a rehearsal now reflects the sensors
  a laydown places, so a future adoption step would have a rehearsal worth consulting.
- **Whether a packaged release carries `testdata/tracks/`** stays the open packaging
  question `gungnir-app/src/state.rs` already named; without it a desktop refuses a
  rehearsal by the path it could not read.

# DN-32 Re-observation for a laydown

Closes GAP-105, filed by D-50: a rehearsal cannot move the sensors a laydown declares.
Decided by D-64 (2026-09-16): **the detections a laydown's sensors would have produced are
generated at rehearsal time, inside the production binary** -- chosen by the owner over
committing one pre-generated fixture per laydown, which keeps the dependency rule intact
but covers only laydowns somebody generated fixtures for. Status: **built 2026-09-25
(GAP-105)**, with what the build found recorded as §12, Amendment 1. That choice changes
what the operational binary contains and adds two dependency edges, now (z) and (aa) in
`ARCHITECTURE.md` §7.1, and this workspace's rule is that neither happens before a signed
design. What the owner has signed of this note, and of the amendment, is in
[`../signatures.md`](../signatures.md).

**The owner's answers, 2026-09-17.** Every sub-decision in §5 as recommended: truth at scan
cadence with no interpolation (§5.1), an `entities.json` sidecar (§5.2), an
`environment.json` sidecar (§5.3), and the deployment's own named detection model with a
refusal by name (§5.4); §5.5's frame kept as GAP-045 signed it; and all five containment
mechanisms in §6, the ingest gateway's half coming back to the owner as a code review when
it is built. §10's verification rows were not part of that walk and are not yet agreed as
pass criteria.

**Drafted 2026-09-16, landed here 2026-09-17, and renumbered on the way.** It sat on the
branch `claude/gap105-laydown-placement` while the numbers it had claimed were taken by
other work: `DN-31` by the node approval queue, `D-53` by the GAP-067 walk, and edges (w)
and (x) by GAP-131. Its register edits, written before the gap data moved into
`../mission/gap-analysis/data/`, were dropped rather than ported. What is below is the
design text with those references corrected and nothing else changed;
`../record/2026-09-17/gap-105-laydown-draft-and-dn-31-reuse.md` records the collision.

## 1. The gap, and the rule it meets

A rehearsal (`gungnir-app/src/laydown_rehearsal.rs`, GAP-045) replays a committed
test-track fixture's `detections.jsonl` through a throwaway desktop. Those detections were
captured once, from the sensor positions in the fixture's own `sensors.json`, so selecting
a different laydown changes nothing a rehearsal can show about sensors. Round 1's laydown
`c` moves S2 from (5 719, 1 082) to (10 762, 2 540) and a rehearsal of it replays the same
detections as the current laydown -- which is why round 1 compares laydowns on coverage
arithmetic alone.

GAP-105 as first filed said "extend `gungnir-scenario` to accept a placement override and
thread it through the rehearsal". **That cannot be built.** `gungnir-scenario` describes
itself as "Test/bench dependency only -- never a normal dependency of a production crate",
and `gungnir-app/tests/dependency_graph.rs` enforces it: any edge to `gungnir-scenario`
from a crate outside the verifier layer is a named violation (`scenario_misuse`). The rule
exists for a reason this note has to answer rather than route around: **a generator of
synthetic observations inside a command-and-control binary is a way for invented data to
be mistaken for sensor data.**

## 2. What production may do, and the line it may not cross

**Production re-observes recorded truth. It never generates truth.**

The fixture already records where every target was (`truth.jsonl`) and most of every
sensor's detection model (`sensors.json`: range per signature class, probability of
detection, dropout, noise, field of regard, altitude limits, horizon, latency,
out-of-order delivery, electronic-attack response, false-alarm rate). Three fields the
model reads are not written there -- `moving_only`, `cued` and the sea-state dropout table
-- which §5.4's catalogue export has to carry. What a
laydown changes is only *where a sensor stands* -- so what production needs is the
narrow half of the generator that answers "given this target where the recording says it
was, would this sensor at this position have detected it, and where would it have
reported it?" That is a pure function of recorded state, a sensor model and a position.

What stays in `gungnir-scenario`, test-only, unchanged: entities, motion, phases, routes,
spawning, occlusion schedules, and every other part of making a world. A production binary
that contains the observation model and not the world model can **re-observe a recording
and cannot invent a target**, which is the property the dependency rule was protecting,
stated at the level it actually matters.

## 3. The owning components

| Concern | Crate |
|---|---|
| The observation model: `observe` and its parameter types | **`gungnir-sensor-sim`**, new |
| The world model, motion, the Python-parity generator | `gungnir-scenario` (unchanged role; now depends on the above) |
| Reading a fixture and a laydown, running the rehearsal | `gungnir-app` (`laydown_rehearsal.rs`) |
| The provenance marker on a re-observed detection | `gungnir-model` |
| Refusing a re-observed detection anywhere live | `gungnir-ingest` gateway (**human-owned**) |
| The sidecar files the fixture must add (§5) | both generators: `docs/test-tracks/tools/gen_tracks.py` and `gungnir-scenario` |

**Two edges**, both new: `gungnir-scenario` → `gungnir-sensor-sim` and `gungnir-app` →
`gungnir-sensor-sim`. **Their letters are assigned when this is built, not here.** The draft
proposed (w), and (w) and (x) went to `gungnir-approval` on 2026-09-17 (GAP-131, D-57,
`dependency-edges.md` §17), which is exactly the collision this note was renumbered for.
Whoever builds this takes the next free letters and writes their own section.

**The new crate has a name that says what it is.** Folding the observation model into
`gungnir-sensor-management` was considered and refused: that crate commands real sensors,
and a module there that fabricates their output would sit one import away from the code
that talks to hardware. A crate called `-sim` is visible as simulation in every manifest
and every dependency graph that names it.

## 4. The extraction

The observation model today is one block inside `gungnir-scenario/src/tracks.rs::run`,
roughly lines 1358 to 1480: signature class, range band, horizon, altitude window, field
of regard, moving-only and cued gates, dropout, the detection draw, measurement noise along
line-of-sight, cross-range and height, sensor bias, the spoof offset, latency and
out-of-order delivery, then false alarms.

It moves to `gungnir-sensor-sim` as a function over explicit inputs, with `Num` and
`PythonRandom` moving with it (both pure, and both needed for parity):

```rust
/// One scan of one sensor over one target, as the recording has the target.
/// `None` is "not detected this scan", for any of the reasons the model has.
pub fn observe(
    sensor: &SensorParams,
    sensor_position: [f64; 3],
    scan: &ScanContext,          // scan time, dropout multipliers, skew, sea state
    target: &TargetState,        // position, velocity, alive, occluded, signature, flags
    rng: &mut PythonRandom,
) -> Option<Observation>;

pub fn false_alarms(sensor: &SensorParams, sensor_position: [f64; 3], scan: &ScanContext,
                    rng: &mut PythonRandom) -> Vec<Observation>;
```

**The extraction is gated by the check this workspace already has.**
`gungnir-scenario/tests/reference_parity.rs` reproduces all ten committed sample sets byte
for byte against the Python reference generator. The extraction must leave that test
passing **unchanged** -- same draws, in the same order, from the same stream -- which makes
"the production observation model is the verified one" a byte-exact fact rather than an
intention.

## 5. What a fixture does not yet record, and a decision for each

Re-observation needs four things production cannot reach today. Each is a separate
decision; the recommendation is given for each and the owner's signature covers all four.

### 5.1 Truth at scan cadence

`truth.jsonl` is written every `truth_tick_s`, which is 2 s in all ten sample sets, while
sensors scan every 0.5 to 10 s -- and nine of the ten sets have a sensor scanning every
0.5 s (TT-06's fastest is 3 s). Most scans therefore fall between truth samples.

**Recommended: the generators write truth at the finest scan period any sensor in the
scenario uses, and a rehearsal refuses to interpolate.** Interpolating would observe a
target at a place the recording never put it -- linearly between two samples of a
manoeuvring track -- and report the result as what the laydown would have seen. The cost is
fixture size, by the ratio of the old tick to the new one -- 4x for the nine sets with a
0.5 s sensor, and it is paid in `testdata/`, not in the binary. *Rejected: interpolation, and
subsampling scans to the truth tick (which would change every sensor's effective update
rate and so its detection count).*

### 5.2 Signature classes and per-entity flags

A truth record names a `platform` (`shahed-136`), and the range a sensor detects it at
depends on that platform's radar, IR or acoustic class, which lives in the YAML catalogue.
YAML parsing is admitted for `gungnir-scenario` only (D-31). Decoy status, intermittent
ADS-B and AIS spoof offsets come from the scenario definition and are in no fixture file.

**Recommended: both generators emit an `entities.json` sidecar** -- per entity, its
signature classes and the flags `observe` reads -- so production reads JSON it already
parses and never touches the catalogue. *Rejected: admitting a YAML crate to production
(a §2.9 change for one consumer), and resolving classes from `class` alone (it names a
threat class, not a signature).*

### 5.3 Environment

Electronic-attack windows (jamming skew, dropout multipliers) and sea state change
detection per scan, and are scenario inputs rather than fixture output.

**Recommended: an `environment.json` sidecar** with the time windows `observe`'s
`ScanContext` needs, emitted by both generators. `events.jsonl` stays the human-readable
narrative it is.

### 5.4 Whose sensor model

**This is the sub-decision most able to produce a comparison that is wrong without looking
wrong.** A laydown places the *deployment's* sensors, by `SensorId`. The fixture has its
own sensors with their own ids and models, and the ids coincide by accident: round 1's
sensor 2 and TT-01's sensor 2 are different sensors with different detection models. The
existing rehearsal already matches by id for resources. Doing the same for sensors would
move the fixture's sensor 2 to where the deployment's sensor 2 stands and report the
result -- a comparison of one sensor's model at another sensor's position, with nothing on
screen to say so.

**Recommended: re-observation uses the deployment's own sensor, and every deployment
sensor names its detection model.** `SensorConfig` gains `detection_model: Option<String>`,
a sensor type from the test-track sensor catalogue (`radar.long`, `eo-ir`, `acoustic`, ...),
whose parameters reach production through a JSON export of `sensors.yaml` generated
alongside the fixtures. A rehearsal **refuses by name** a laydown that places a sensor
with no detection model, and never borrows a fixture sensor that happens to share its id.
*Rejected: matching by id (above), and inferring a model from `modality` and
`max_range_m` (a modality is not a detection model; range alone has no per-class bands,
no probability of detection and no noise).*

### 5.5 Frame

`laydown_rehearsal.rs` already reads a laydown's `position_enu` about the fixture's own
fictional origin, a convention signed for GAP-045 and explained in that module. This note
keeps it, and states its consequence rather than changing it: a laydown is rehearsed as an
arrangement relative to the fixture's origin, not as a placement at the deployment's real
coordinates. Changing it would need the fixture's truth moved to the deployment's origin,
which is a separate decision this note does not take.

## 6. Containment: a re-observed detection must never be mistakable for a live one

Five mechanisms, each independent, so that no single mistake reaches the picture:

1. **A typed marker.** `Provenance` gains `rehearsal: Option<RehearsalOrigin>`
   (`{ scenario: TestTrackNumber, laydown: LaydownId, seed: u64 }`). Every `Observation`
   `gungnir-sensor-sim` returns carries it, set by the function rather than by its caller,
   so there is no code path that produces an unmarked one.
2. **The live gateway refuses it.** `IngestGateway` rejects any detection whose provenance
   carries `rehearsal`, with a named rejection counted like every other. The rehearsal's
   throwaway gateway is constructed with an explicit opt-in that the live one never is.
   `gungnir-ingest` is human-owned, so this half comes to the owner as a review.
3. **Throwaway state only.** Already true: a rehearsal runs on its own `AppState` with its
   own journal directory, and nothing it produces reaches the live desktop's picture or
   record (GAP-045).
4. **The dependency rule, narrowed rather than deleted.** `dependency_graph.rs` keeps
   `scenario_misuse` exactly as it is and gains `sensor_sim_misuse`: `gungnir-sensor-sim`
   may be a dependency of `gungnir-scenario`, `gungnir-app` and the verifier layer, and
   of nothing else -- not `gungnir-node`, `gungnir-ingest`, `gungnir-tracking-service`,
   `gungnir-api` or `gungnir-remote`. A node that cannot rehearse cannot leak a rehearsal.
5. **One module may call it.** Within `gungnir-app`, only `laydown_rehearsal.rs` may name
   `gungnir_sensor_sim`, checked by a source test in `architecture_compliance.rs`.

And on screen, PN-16 labels every rehearsal result as **re-observed from a recording**,
naming the fixture and the detection model used for each sensor.

## 7. Determinism, and what it is not identical to

A rehearsal seeds its stream from the fixture's seed and the laydown's identifier, so the
same laydown rehearsed twice produces the same detections.

**It does not reproduce the fixture's own `detections.jsonl`, even for the current
laydown**, and that is stated here so that nobody reads a difference as a defect. The
generator draws motion and observation from one interleaved stream -- phase durations,
stops and weave between detection draws -- and a re-observation of recorded truth makes
none of the motion draws, so its stream is at a different position at every scan. Equal
statistics, not equal lines, is the correct expectation, and §10's second row checks
exactly that.

## 8. Behaviour

1. A planner selects a scenario and a laydown on PN-16 and runs a rehearsal.
2. `laydown_rehearsal.rs` loads the fixture's truth, `entities.json`, `environment.json`
   and metadata; resolves each laydown sensor's detection model, refusing by name any it
   cannot; and places sensors per §5.5.
3. It steps scan time, calls `observe` and `false_alarms` for every sensor and target, and
   feeds the marked observations to the throwaway desktop's opted-in gateway.
4. The run reports what `RehearsalRecord` already reports -- tracks formed, decisions raised
   and expired -- now as seen by *this* laydown's sensors, plus per-sensor detection counts
   so a planner can see which sensor a difference came from.

## 9. What this note deliberately does not do

* **No truth generation in production.** §2.
* **No arbitrary scenario authoring.** Rehearsals run against committed fixtures only.
* **No adoption workflow.** GAP-107 owns that question.
* **No change to where the deployment is.** §5.5.
* **No byte-identical reproduction of a fixture's detections.** §7.

## 10. Verification

| Row | Method | Pass criterion |
|---|---|---|
| Extraction preserves the verified generator | `gungnir-scenario/tests/reference_parity.rs`, unchanged | All ten sample sets byte-identical to the Python reference |
| Re-observation matches the recording statistically | Re-observe each sample set's truth with its own sensors at their own positions | Per sensor, detection count within 2σ of the recording's; per axis, measurement-residual variance within 2σ of the model's noise -- the same terms as §1's statistical self-check |
| Geometry moves detections | Property test: move a sensor so a target leaves, then enters, its range band | Detections of that target stop, then start, and nothing else changes |
| A laydown sensor with no model is refused | Unit test | Named refusal; no rehearsal runs |
| Containment | Live gateway fed a marked observation; a manifest adding a forbidden edge; a second module naming the crate | Rejected and counted; `sensor_sim_misuse` names the edge; the source test names the module |
| Round 1 laydown `c` | Rehearse `current` and `c` over the round-1 scenario | S2's per-sensor detection counts differ, and the record says which sensor the difference came from |

## 11. Interface, configuration and data-format delta

* New crate `gungnir-sensor-sim`; its two edges as §3, lettered when built; `ARCHITECTURE.md`
  §7.1 and the top-level graph updated with it.
* `gungnir-model`: `Provenance::rehearsal: Option<RehearsalOrigin>`, `#[serde(default)]`, so
  every existing record deserialises unchanged.
* `gungnir-config`: `SensorConfig::detection_model: Option<String>`, validated against the
  exported sensor catalogue.
* `gungnir-ingest` (human-owned): the gateway's rehearsal refusal and opt-in.
* `docs/test-tracks/data-format.md`: truth at scan cadence (§5.1), `entities.json` (§5.2),
  `environment.json` (§5.3), and a JSON export of `sensors.yaml` (§5.4). All ten sample sets
  regenerated by both generators and re-validated.
* No API route. No change to what `gungnir-node` builds.

## 12. Amendment 1 -- what the build found (2026-09-25)

Built as GAP-105, under the owner's delegation of 2026-09-25. Where the build found this
note wrong or silent it took a decision, D-72 to D-74, and recorded the rest here; the
reasoning is in `../record/2026-09-25/gap-105-re-observation-for-a-laydown-built.md`. What
the owner has signed of this amendment is in [`../signatures.md`](../signatures.md). The
sections above are unchanged, so a reader holding the walked text still has it.

**§5.1, truth at scan cadence -- replaced by the recording's own tick (D-72).** The
premise was that most scans fall between truth samples, so a re-observation must either
interpolate or be given truth at the finest scan period. Both generators in fact step each
entity once per truth tick and then run every scan that fell due since the previous tick
against the state just stepped to: every detection a recording holds was made of the target
**as the truth record of the first tick at or after its scan has it**. Re-observing by that
same rule is the recording's own observation rule, not an interpolation -- no position is
ever computed between two records -- and `gungnir_sensor_sim::reobserve` refuses a truth
record off the tick grid rather than place it (`ReObservationError::NotOnTick`). A finer
tick would not have delivered what §5.1 wanted either: every sensor's first scan is at a
random phase, so scans would still fall between 0.5 s samples; and it would have rewritten
all ten committed sample sets, which gated rows elsewhere in the verification table
measure, for no gain in the property §5.1 protects. The sets keep their 2 s tick and their
four data files are unchanged byte for byte.

**§5.2, the entity sidecar, needed three facts the section did not list**: the entity's
`spawn_s` and `occluded_window_s`, because occlusion is judged at the scan's own time, not
the tick's; and `destroyed_at_s`, because an entity destroyed by another's dash keeps a
truth record reading alive for the tick it died in, written before the strike, and no scan
at that tick saw it. The emission class is resolved once per entity in `sensors.yaml`'s
`emission_map` order. **Writing it for every entity found a defect in the Rust port of the
generator**: its `emission_map` was a sorted map, so "control and video link", listed under
both `datalink` and `control`, resolved to `control` where the reference resolves
`datalink`. No committed detection showed it; `gungnir-scenario/tests/sidecar_parity.rs`
did on its first comparison, and the map keeps the file's order now.

**§2's aside that `sensors.json` does not carry `moving_only`, `cued` or the sea-state
table was wrong** -- it carries whatever the type declares -- and is moot: §5.4's catalogue
export is what a rehearsal reads, at `testdata/tracks/sensor-models.json`, beside the
sample sets and written by both generators byte for byte.

**§5.3, the environment sidecar -- the recording's sensor-specific events are counted, not
applied, to a deployment's sensors (D-73).** Losses and electronic-attack windows name the
recording's own sensors, and §5.4's reasoning against matching by identifier applies to
them exactly: a jamming window on the recording's sensor 1 is not a fact about the
deployment's sensor 1. Transferring them by sensor type was considered and refused, since
jamming depends on where the jammer is, which the recording does not state. The sea state
names no sensor and is applied to every placed sensor. The record counts what was not
applied, and PN-16 says so. When the placed sensors *are* the recording's own, as in §10's
statistical row, every event applies by identifier as the generator applied it.

**§5.4, validation against the catalogue, happens where the catalogue is read.**
`gungnir-config` checks that a `detection_model` has the form of a catalogue identifier and
refuses an empty or spaced one at load; the rehearsal resolves the name against the export
it reads and refuses an unknown one by name. A baseline does not know where a packaged
release keeps the catalogue, and a desktop that cannot read it refuses the rehearsal by the
path it could not read. A sensor the laydown places in a mode that does not observe
(standby, calibrating, offline) needs no model and is reported as not observing, and a
laydown with no observing sensor is refused rather than rehearsed as seeing nothing.

**§6 mechanism 1, the marker, crosses a layer.** `gungnir-sensor-sim` sits in the tracking
core, beneath `gungnir-scenario`, which depends on it; it may not name `gungnir-model`'s
`RehearsalOrigin` without a core crate reaching up into the foundation. So every
observation it returns carries its own `SimulationMark` (recording, placement, seed), set
by `observe` and `false_alarms` and by nothing else -- the type's fields are private -- and
`gungnir-app/src/laydown_rehearsal.rs` converts it into `Provenance::rehearsal`. The
conversion is total and checked: a mark naming another recording or laydown than the run's
converts to nothing and the detection is not fed. `Provenance::rehearsal` is left out of a
record when `None`, so every existing journal, wire message and fixture is written exactly
as before.

**§6 mechanism 2, the gateway's refusal**, counts a refused detection as a quarantine, with
`IngestStats::rehearsal_refused` naming which of the quarantines were a rehearsal's; the
gateway is human-owned. **One further boundary was found and closed**: the Arrow exchange
schema has no column for the mark, so `gungnir_interop::detections_to_record_batch` refuses
a marked detection rather than write it with the mark stripped.

**§6 mechanism 4's edges are (z) `gungnir-scenario` → `gungnir-sensor-sim` and (aa)
`gungnir-app` → `gungnir-sensor-sim`**, the next two letters after (y)
(`dependency-edges.md` §19).

**§7, determinism -- common random numbers (D-74).** §7 seeded the stream from the
recording's seed and the laydown's identifier. Seeding by the laydown makes two laydowns
draw differently everywhere, so a sensor they place identically would detect differently
under each, and a comparison would name sensors nothing had moved -- the opposite of §10's
round-1 row. Every draw is instead keyed by the recording's seed and **which sensor and
which target** it concerns: one stream per sensor for its scan phase, one per sensor for
its false alarms, one per sensor and target for detection. The same laydown rehearsed twice
still produces the same detections, which is what §7 asked; two laydowns differ only where
their geometry does. The laydown still travels on every mark.

**§10, the statistical row, reads "detection" as a detection of a recorded target**, and
holds false alarms to the configured clutter rate instead, pooled, as §1's self-check holds
its own. A false alarm reads nothing of the truth or of where anything stands, so a
recording's clutter count is one draw from the same rate and there is nothing for a
re-observation to disagree with; and one of them is a tail draw -- TT-08's sensor 13
recorded 21 false alarms against the 36 its model expects, 2.5σ low -- which a per-sensor
comparison with the recording would read as a fault in the re-observation. The residual
row pools every sensor's residuals, normalised by that sensor's σ on that axis, into one
test per axis: two hundred tests at 2σ would each fail about one time in twenty by
construction.

**§10, the round-1 row, has no committed recording to run over (GAP-147).** Under §5.5's
frame round 1's harbour and plant sit at the recordings' origin, and none of the ten sample
sets brings a target within 25 km of it before its excerpt ends, so round 1's two
short-range radars re-observe nothing of any of them, under any laydown. The row's test
therefore writes the smallest recording that asks its question -- three drones down round
1's own declared approach, in round 1's own frame -- and rehearses round 1's own baseline,
laydowns and detection models over it: S2's detections differ between `current` and `c`
(1 241 and 1 260), S1's are identical, and the comparison names S2 alone as moved. A
committed round-1 recording is scenario content, which the owner has decided for round 1
before (D-28), and is GAP-147's. PN-16 says, when a rehearsal re-observes nothing, that the
recording's targets never come within the placed sensors' range bands, rather than leaving
a column of zeros to be read as a fault.

**A sensor's azimuth sector is honoured (GAP-118, D-84, merged beside this build).** A
deployment sensor may declare the bearings it can see and a laydown placement may re-aim
it; a rehearsal re-observes with the placement's sector if it states one, else the
declared one, turned into the recording's frame where the sensor stands. The sector gates
**on top of** the detection model's own field of regard -- the model says what a sensor of
that type sees, the sector says which way this one faces -- and a target outside it is
skipped before any draw, so D-74's streams stay aligned whichever way the sensor faces;
clutter is still drawn all round and what falls outside the sector is dropped. A re-aim
counts as a move when two rehearsals are compared. Tested in `gungnir-sensor-sim`'s
`a_sensor_sees_only_inside_its_sector_and_facing_it_changes_nothing_else`.

**Round 1's laydown `c` is kept, deliberately.** It was added to give the coverage
comparison a pair that differs in siting (D-28), and it still does: PN-16's coverage
columns read it exactly as before. It is now also the case §10's round-1 row is written
against -- the one laydown that moves a sensor and nothing else -- and `b`, which moves a
battery and no sensor, is the contrast that shows a rehearsal attributes a difference only
to what moved. Retiring it would take away both.

## Traceability

GAP-105 (the gap), D-50 (which scoped it), D-64 (the choice this note implements), DN-26
§7 (which excluded rehearsal from the laydown note), GAP-045 (the rehearsal this extends),
GAP-016 (the generator and its parity test), D-31 (YAML in `gungnir-scenario` only),
`gungnir-app/tests/dependency_graph.rs` (`scenario_misuse`).

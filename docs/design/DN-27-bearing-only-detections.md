# DN-27 Bearing-only detections

Unblocks the acoustic, passive-RF and spotter halves of GAP-001, and the sensor-side half
of GAP-004. Status: **proposed 2026-09-06, unsigned.**

**Built 2026-09-06 and gated; still unsigned, and the note is unchanged below.** What was
built, so a reader is not left comparing a specification against a guess:

| Section | Where it landed | What is *not* there |
|---|---|---|
| §4 the type | `gungnir_model::Measurement`, `SCHEMA_VERSION` 2 to 3 | -- |
| §5 rule 1 | `gungnir_fusion_async::{BearingDetection, FusionPipeline::offer_bearing}`, with `gungnir_filters::{BearingOnly, AzimuthElevation}` for the update. The prohibition is a **type boundary**: no function takes a bearing and creates a track | A bearing is applied at the pipeline's current cursor, not retrodicted; one outside the reorder horizon is refused and counted |
| §5 rule 2, §6 | `gungnir_coord::cross_bearings`, refusing below `DEFAULT_MINIMUM_CROSSING_ANGLE_RAD` (15 degrees, chosen there with its reason) | Nothing calls it yet: the pairing that would feed it is §9's open row |
| §5 rule 3 | `PipelineSettings::bearing_retention_s`, `FusionPipeline::retained_bearings` | -- |
| §7 the display | **Not built.** No ray is drawn; `gungnir-ui` and `gungnir-viewport3d` are untouched | The whole of §7 |
| §8 migration | All six producers, plus the Arrow codec and the committed test-track sample sets, which §8 did not list and which also carry the shape | -- |
| §10 verification | The three rows, in `../verification-capability-table.md` §1, all passing | -- |
| The spotter adapter | `gungnir_ingest::adapters::sapient`, registering `NODE_TYPE_HUMAN` (`external-standards.md` §7) | The **binary** SAPIENT wire format: the adapter reads the protobuf JSON mapping, because a protobuf runtime is not in the workspace dependency set |

**One thing built here is not wired**, and saying so is the point of this row:
`gungnir-tracking-service` refuses a bearing with `SubmitError::NotAPosition` rather than
offering it to `FusionPipeline::offer_bearing`, because a bearing needs the reporting
sensor's position in the local frame and `DetectionView` carries a `SensorId` and no
position. Nothing in this workspace resolves one for that service. So the gateway accepts
a spotter's bearing, the record keeps it, and no track is refined by one yet.

## 1. The gap, and why it blocks three feeds rather than one

`gungnir_model::DetectionView` carries `measurement: Vector3<f64>`, a position in the
local ENU frame. Every adapter this system has produces one, because radar and AIS both
report a position.

**Acoustic arrays, passive radio-frequency receivers and people do not.** An acoustic
array gives a direction of arrival. A direction finder gives a bearing and often an
elevation. A person with a compass and a pair of binoculars gives a bearing and, if they
are lucky and have a laser rangefinder, a range. `docs/design/external-standards.md` §7
records that SAPIENT's `RangeBearing` makes azimuth, elevation **and range** each optional
with a paired error, precisely because the sensors it was written for behave this way.

So all three feeds are blocked on the same question, and it is not a question about
adapters. **The adapter is the easy part. What is hard is what the system is allowed to
believe when it is told a direction and not a place.**

## 2. The one thing this note exists to forbid

**A bearing must never be turned into a position by assuming a range.**

It is the obvious shortcut and it is available in three forms, all of which have been used
in real systems and all of which are refused here:

* assume a nominal range, so every acoustic detection appears at 2 km;
* project the bearing onto the terrain and take the intersection, which is a position
  whose error is the terrain's slope and which is simply wrong for anything airborne;
* project onto a defended asset, so the picture shows what the operator fears.

Each produces a `DetectionView` that is a valid value of its type, enters the tracker
without complaint, initiates a track, and is drawn as a symbol at a place nothing is. That
is the confidently-wrong answer `CLAUDE.md` exists to prevent, and it is worse here than
almost anywhere else in this system, because the operator has no way to tell the invented
position from a measured one.

## 3. The owning components

| Concern | Crate |
|---|---|
| The measurement's shape | `gungnir-model` |
| Angles to a Cartesian measurement and its covariance | `gungnir-coord` |
| What the tracker may do with a bearing | `gungnir-fusion-async` (human-owned) |
| The measurement model the filter already has | `gungnir-filters` (human-owned) |
| The adapters | `gungnir-ingest` (the gateway; human-owned) |
| Drawing one | `gungnir-ui`, `gungnir-viewport3d` |

**This note adds no dependency edge.**

## 4. The type

`DetectionView.measurement` becomes an enumeration. It is a breaking change to the
canonical model and there is no smaller one: an optional range beside a position would let
a producer set both, or neither, and the reader would have to guess.

```rust
/// What a sensor actually measured. **Not always a position**, and the type says so
/// rather than letting an adapter invent the difference.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Measurement {
    /// A position in the local ENU frame, metres. What radar and AIS report.
    Position {
        enu: nalgebra::Vector3<f64>,
        /// Per-axis variance, metres squared. **Required**: a position with no stated
        /// error is one the tracker has to guess a gate for, and it guesses generously.
        variance_m2: [f64; 3],
    },
    /// Range, azimuth and elevation from the sensor. What a radar reports natively
    /// before anyone converts it, and what `gungnir_filters::RangeAzimuthElevation`
    /// already models.
    RangeAzimuthElevation {
        range_m: f64,
        azimuth_rad: f64,
        elevation_rad: f64,
        variance: [f64; 3],
    },
    /// A direction and no range. What an acoustic array, a direction finder and a
    /// person report.
    ///
    /// `elevation_rad` is optional because a ground-based direction finder frequently
    /// has none, and a missing elevation is not a zero one: zero means the horizon.
    Bearing {
        azimuth_rad: f64,
        elevation_rad: Option<f64>,
        /// Angular one-sigma error, radians. Small here means a large cross-range error
        /// far away; see §6.
        azimuth_variance_rad2: f64,
        elevation_variance_rad2: Option<f64>,
    },
}
```

Azimuth follows the convention `gungnir_filters::RangeAzimuthElevation` already uses:
`atan2(east, north)`, a compass bearing, zero at north and increasing to the east. It is
restated here because the one thing more expensive than an unstated convention is two
components each assuming a different one.

**Every variant carries its error.** The present `DetectionView` does not, and the gate
downstream has to assume one. That is tolerable for a position from a radar whose accuracy
the baseline states; it is not tolerable for a bearing, where the error *is* the
information.

## 5. What the tracker may do with a bearing, and what it may not

Three rules. The first is the one that matters.

**Rule 1: a bearing may update a track and may not initiate one.**

A single bearing does not determine a position, so there is no state for a new track to
start in. `gungnir-filters` already has the machinery for the update -- the extended and
unscented filters take a `MeasurementModel`, and a bearing-only model is that model with
the range row removed -- so an existing track's estimate is refined by a bearing exactly as
it is by a range-azimuth-elevation report, with no new mathematics.

Initiation is the part that has no answer. A single fixed sensor cannot localise from
bearings at all without moving: the problem is unobservable, and a filter given a sequence
of bearings from one fixed point will happily converge to a confident answer at the wrong
range. **That failure is silent and it looks exactly like success**, which is why the rule
is a prohibition and not a tuning parameter.

**Rule 2: two or more bearings that cross may initiate a track, and the crossing carries
the covariance the geometry gives it.**

Two bearings from separated sensors intersect at a point. That is a position and it may
initiate. Its covariance is **not** isotropic and must not be modelled as such: the error
ellipse is long along the bisector and short across it, and it degenerates as the crossing
angle goes to zero -- two nearly parallel bearings determine almost nothing about range
while determining direction well. A crossing below a stated minimum angle is **refused**
rather than initiated with a very large covariance, because a track whose position
uncertainty is tens of kilometres long is not a track, and drawing it as one is the same
error as §2's.

Associating which bearing crosses which is the hard part and is a genuine data-association
problem, not a geometric one. With `n` bearings from two sensors there are `n²` candidate
crossings and at most `n` are real; the rest are ghosts, and a ghost crossing is
indistinguishable from a real one on geometry alone. **This note does not solve that**, and
that is deliberate: it is the JPDA-shaped problem `gungnir-association` already owns, and
naming it here as an open row is more honest than specifying a rule that would not work.

**Rule 3: a bearing that updates nothing is retained and shown, not dropped.**

A direction with no track behind it is the acoustic array's ordinary output when something
is out there that the radar cannot see, and it is exactly the report an operator most needs.
It stays in the picture as a bearing, for a stated lifetime, and is drawn as §7 says.

## 6. Why the angular error must survive the conversion

An angular error is a constant; the cross-range error it implies is not. One degree at
1 km is 17 m across. The same degree at 30 km is 520 m. A conversion that turns a bearing
into anything and keeps a fixed positional variance is wrong at every range but one.

So where a bearing becomes Cartesian -- at a crossing, under rule 2 -- the covariance is
built from the geometry and not from a constant:

```text
σ_cross ≈ r · σ_azimuth        (metres, for small angles)
σ_along  from the crossing angle, and unbounded as that angle goes to zero
```

`gungnir-coord` owns the conversion, because it owns every other frame transformation and
because a conversion that loses an error term is the kind of defect that is found years
later in a gate that had always been green.

## 7. What a bearing looks like on the display

**A ray, not a symbol.** A symbol is a claim about where something is; a bearing is a claim
about a direction. It is drawn from the sensor along the azimuth, widening with the angular
error, and it does not terminate -- a drawn end point is a range nobody measured.

A crossing that initiated a track under rule 2 draws as an ordinary track with its real
error ellipse, which will be visibly elongated, and that elongation is information rather
than an artefact to be hidden.

An acoustic or radio-frequency detection that classifies but does not localise -- "a
gunshot, bearing 037" -- belongs on PN-09 and on the alert list, not as a contact on the
map.

## 8. Migration

`DetectionView.measurement` changing shape touches every producer. There are six today:
the ASTERIX adapter, the AIS adapter, the peer adapter, the recorded adapter, the gateway's
own tests and the scenario generator. Each produces a position and each becomes
`Measurement::Position` with the variance the baseline already states for that sensor, so
the change is mechanical for all six and no behaviour moves.

**`SCHEMA_VERSION` must be bumped.** This is not additive: a consumer holding the old shape
cannot read the new one, and the exact-match version rule
(`gungnir-remote/tests/wire_conformance.rs`) will refuse a peer one version out, which is
the correct outcome and the reason that rule exists.

## 9. What this note deliberately does not do

* **No bearing-only initiation from one sensor**, however many reports. See rule 1.
* **No ghost resolution.** Which crossing is real is `gungnir-association`'s problem and
  wants its own row and its own oracle.
* **No terrain intersection**, which §2 refuses outright.
* **No triangulation across more than two sensors.** Three bearings rarely meet at a point
  and the least-squares fit that resolves them is a different piece of work with a
  different error model.

## 10. Verification

Three new rows in `docs/verification-capability-table.md` §1:

| Component | Test | Method | Criterion |
|---|---|---|---|
| `gungnir-model` bearing measurement | `gungnir-model/tests/measurement.rs` | every variant round-trips; a bearing with no elevation is distinguishable from one with zero elevation | exact round-trip; the two are not equal |
| `gungnir-coord` bearing crossing | `gungnir-coord/tests/crossing.rs` | two bearings of known geometry, and a pair below the minimum crossing angle | crossing position within 1e-6 m of the closed form; the shallow pair is refused, not returned with a large covariance |
| `gungnir-fusion-async` bearing update | `gungnir-fusion-async/tests/bearing.rs` | a bearing offered to an empty pipeline, and to a pipeline holding a track it gates into | **initiates nothing** in the first case; refines the estimate in the second, with the cross-range error scaling with range |

The first row's second half is the one worth writing carefully: an optional elevation that
serialises indistinguishably from zero would put every acoustic detection on the horizon.

## Traceability

CAP-1.1 (sensor adapters), CAP-1.3 (sensor tasking), MT-02. `../design/external-standards.md`
§§7 and 9, which pin the specifications these feeds arrive under. Related notes: DN-11
(sensor control and tasking), DN-16 (peer sources -- whose launch warning is the other
message in this system that is deliberately not a track), DN-25 (Cursor-on-Target, whose
`ReportedPosition` is likewise deliberately not a track).

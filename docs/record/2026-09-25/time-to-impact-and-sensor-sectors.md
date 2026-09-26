# Time to impact and sensor sectors

GAP-124 and GAP-118 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-01 §10 and DN-12 §9, D-83 and D-84. Both gaps were filed by the GAP-067 walk
([`../2026-09-16/gap-067-walk.md`](../2026-09-16/gap-067-walk.md)), which held the two rows
they stand behind. Taken under the owner's delegation of 2026-09-25.

## GAP-124: what was wrong

The `gungnir-assessment` Risk scoring row's criterion is "score monotonic in
time-to-impact". The asset-list score was proximity times a closing factor of 1.0 or 0.5
times priority and lethality. It read how far a track was and whether it was closing at
all, never how soon it would arrive, so a track 8 km out at 200 m/s -- 40 s from the asset
-- scored below one 1 km out at 5 m/s, 200 s away. The row's only fixture moved range at
one constant speed, where range and time to impact move together, so it could not see it.

## GAP-124: what was decided, and why not the alternatives

**Which time to impact.** Two were on the table: range to the boundary over the closing
speed along the line of sight, which DN-01 §5 and `RiskScore` already defined and DN-03's
warnings already read; and the time to the closest point of approach, which GAP-020's
predictor computes. The second was rejected. It is blind to the miss distance: a track
crossing 20 km off with its closest approach 10 s away would be "10 s from impact", and
DN-03 fires an impact warning on exactly that number, keeping the pass-close obligation
for tracks that miss. And it is discontinuous: the moment a crossing track passes its
closest point the time jumps from zero to undefined and the score halves in one tick. The
radial definition is exact for a track aimed at the asset, grows without bound as a track
turns to pass -- which is the miss distance entering by itself -- and is continuous
through the pass. The closest approach is still on the exposure, and the exposure now gets
it from the predictor's own routine (`prediction::closest_on_course`) instead of its own
copy, which is the reuse the gap asked for.

**How it combines with range.** The obvious move, multiplying a time term into the old
product, was rejected because it leaves the defect in place near ties: two tracks arriving
within a few seconds of each other are then ordered by range, and a far-fast track that
arrives sooner loses to a near-slow one. The only form in which the score is monotonic in
time to impact for every pair of closing tracks, not only the fixture's, is one in which a
closing track's kinematic factor depends on its time to impact alone. So it does:
`1/2 + u/2` with urgency `u = τ / (τ + T)`, in `(1/2, 1]`. A track that is not closing has
no time to impact and keeps what it had, half its proximity, in `[0, 1/2]`. The
consequence is stated rather than hidden: at equal priority and class, every confidently
closing track outranks every track that is not closing -- which is what "monotonic in time
to impact" means for a track that has none -- and a hovering contact beside an asset ties
with a slow distant closer rather than beating it. Priority and lethality, which multiply
the factor, separate them in practice; a pass-close warning is DN-03's job, not the
score's.

**What the covariance does.** A step at zero closing speed would promote every
near-stationary contact whose velocity estimate wanders by a metre a second to "inbound"
and drop it again a tick later. The closing speed's own one-sigma, from the velocity block
of the track's covariance along the line of sight, decides how far a track is credited as
closing: `(Φ(z - 2) - Φ(-2)) / (1 - Φ(-2))`, zero at zero, a half at two sigma, one beyond.
The factor is the blend of the closing and not-closing values by that credit, so it is
continuous as a track turns and a noise-level closer is not promoted. A covariance that
cannot give a sigma -- not finite, or negative along the line -- credits the estimate
fully, because down-rating a closing track for a bookkeeping fault is the unsafe direction.
`Φ` is the Numerical Recipes Chebyshev fit to `erfc`, fractional error below 1.2e-7.

**The scale** `τ` is `assessment.urgency_half_time_s`, 60 s by default and refused unless
finite and positive. It is a deployment's, because "soon" at a counter-UAS site and at a
port are different; it changes how steeply the score falls, never the order.

**What it survives.** Nothing divides by the closing speed: urgency is
`1 / (1 + r / (τ v))`, with an underflowing product giving no urgency rather than 0/0. A
position or velocity that is not a finite number gives no factor, and the assessor reports
no exposure rather than a NaN -- which `total_cmp` would have sorted to the top of PN-17's
triage and fed into the reward matrix. That guarantee is under `agentic-workflow.md`'s
numerical-stability clause and is human-owned; see [`../../signatures.md`](../../signatures.md).

**On screen.** `RiskScore::kinematics` carries the terms, and PN-04's evidence card shows
the closing speed and time to impact with the urgency, the closing confidence, the proximity
and the factor, read from the score rather than recomputed. The line it replaced said
"closing, N s to impact" with a weight of 1.0 whatever N was.

## GAP-124: what the tests hold

`gungnir-assessment/tests/time_to_impact.rs` moves time to impact independently of range:
one range at two closing speeds; far-fast against near-slow, including a near tie 60.0 s
against 60.3 s across a factor of five in range; a grid of eight ranges, seven speeds and
four bearings against a point and an area asset, sorted by time to impact, asserting the
score never rises and falls wherever the time differs by more than a percent; one track
flown in; a track not closing taking no urgency; a less certain closing speed never
earning more; a NaN state not scored. `kinematics.rs`'s own tests hold the factor finite
and in `[0, 1]` at zero, subnormal and enormous closing speeds, a NaN and a negative
covariance, a track on the asset's centre; continuity through zero closing speed; and `Φ`
against its table. PN-04's lines are held in `gungnir-app/tests/asset_exposure_panels.rs`,
including that the baseline's half-time is the scorer's.

## GAP-118: what was wrong

The `gungnir-analytics` Coverage accuracy row asks for range, bearing and elevation limits
recovered from computed coverage within 1 percent and 0.1 degree. `CoverageVolume` had a
range and a lower elevation limit and no azimuth, so bearing had nothing to recover, and a
panel radar, a fixed camera or a sensor masked by its own mast was drawn, counted and
compared as covering the full circle.

## GAP-118: what was decided

The alternative the gap offered -- take bearing out of the criterion -- was rejected: real
sensors have sectors, and coverage counted where a sensor cannot look is the failure DN-12
exists to prevent. `gungnir_model::AzimuthSector` is a boresight and a width in `(0, 2π]`.
A boresight and width rather than two edges because the width is what validation bounds,
and a sector across north then needs no special case; containment is the signed offset from
the boresight, so 350 degrees wide 40 covers 330 through north to 10.

**Against true north at the sensor**, in a baseline, because that is what a survey states.
The local frame's north is true north only at the origin; elsewhere the meridians converge,
and the frame bearing of true north at a sensor 39 km east of an origin at 45 degrees north
is 0.35 degree off -- more than three times the criterion. `LocalFrame::true_north_at`
takes it numerically from the frame's own conversion, and every path that builds a volume
turns the sector by it: `coverage_from_registry` (which now takes the frame, not a
conversion closure), `volume_of` for DN-13's candidates, PN-16's
`laydown_coverage_volumes`, and PN-11's circles.

**On the sensor, and optionally on the placement.** `SensorConfig.azimuth_sector` states
the sensor's sector; `SensorPlacement.azimuth_sector` lets a laydown re-aim a sensor it
moves, and a placement without one inherits the declaration's. **Absent is the full
circle**, on both, so every baseline written before this means what it meant. Validation
refuses a non-finite boresight or a width outside `(0, 2π]`, naming the sensor or the
laydown and placement; `analytics.coverage_min_elevation_rad` had no check at all and is
now refused outside `[-π/2, π/2]`, since a NaN there made every point uncovered.

**Drawn as a wedge.** PN-11 draws a sectored sensor from its position out to its range
between its edges and back; the full circle, by omission or stated, stays a ring.

## GAP-118: what the fixture found

The approach sampler restarted its spacing at every vertex: a segment shorter than the
spacing contributed no sample, and a longer one dropped its remainder at the end. An
approach drawn with vertices closer than 250 m -- a curved axis, a digitised route -- was
judged by its first point alone. The fixture's arc probes, with vertices every 0.05 degree,
came back with no samples, which is how it showed. Samples are now spaced along the whole
polyline, `k * spacing` for the `k`-th, so a long approach does not drift either. The
coverage figures the existing tests and the round-1 rehearsal assert are unchanged by it.

## GAP-118: what the tests hold

`gungnir-analytics/tests/coverage_accuracy.rs` states six volumes -- a panel radar, a sector
across north, a 7.5 degree sector off the cardinal points, the full circle by omission and
stated, and a sector that is all but a 10 degree notch -- and recovers each limit from
`combined_coverage` with one sensor at a time, so covered samples are single-sensor runs and
the limit lies between two runs. Range along a ray sampled at exactly 1 percent of the
range, the coarsest the row allows, recovered within half of it; sector edges on a circle
at half the range and the lower elevation limit on a vertical arc, both sampled at 0.1
percent, because 1 percent there would be 1.1 degree between samples. A second fixture
declares three sensors geodetically, one 39 km east of the origin, runs them through the
registry and the frame, and turns each recovered edge back to true north before comparing:
removing the rotation fails it at 0.38 degree. The PN-16 path is held in
`gungnir-app/tests/planning_panel.rs`: a radar aimed away from the approach leaves all of it
uncovered, aimed along it covers what the full circle did, and a declared sector is
inherited.

## What is left

GAP-158: a coverage volume's elevation limit is one floor for the whole baseline with no
ceiling, measured against the frame's vertical rather than the sensor's, which tilts about
0.009 degree per kilometre from the origin. Filed rather than built here, because an upper
limit is a second change to every sensor declaration with a question of its own -- what it
defaults to.

Neither row in `../../verification-capability-table.md` is changed: both await the owner's
walk, with these tests as their evidence. The Coverage accuracy row's criterion cell still
says a volume has no azimuth sector; it is the owner's cell, so that is GAP-159 rather than
an edit here, as GAP-154 was for the reconciliation row.

# A sensor's elevation band, a round-1 recording and an unused queue

GAP-158, GAP-147 and GAP-121 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-110, D-111 and D-112, DN-12 §10 and DN-32 §13. D-111 and D-112 were taken under the
owner's delegation of 2026-09-26; D-110 was taken by the owner, directly, on 2026-09-26.

## GAP-158: what was wrong

GAP-118 gave a coverage volume a bearing and left its elevation as it was: one
`analytics.coverage_min_elevation_rad` for every sensor in a baseline, no ceiling, and
measured against the local frame's vertical. A radar masked below 2 degrees and a camera
looking up at 30 degrees had the same floor; a radar's cone of silence overhead was
counted as covered; and a sensor away from the origin had its limit measured against a
vertical that is not its own. The two normals part by the arc between them, about 0.009
degree a kilometre, so beyond about 11 km a stated limit was off by more than the
Coverage accuracy row's 0.1 degree.

## GAP-158: what was decided (D-111), and why not the alternatives

**A band, stated whole.** `gungnir_model::ElevationBand` is a floor and a ceiling in
`[-π/2, π/2]`, the ceiling above the floor. The gap's own question was what an upper limit
defaults to. It defaults to the zenith, `π/2`, which is no ceiling: that is what every
sensor had, so every baseline written before this means what it meant. The alternative of
two independently optional limits was rejected. It lets a placement state a floor and
inherit a ceiling from the declaration, and the two together can make an empty band that
neither place stated, which validation could then only catch after composing them -- and
report against a line nobody wrote. A floor and a ceiling are one survey of one sensor.

**Declared on the sensor and optionally on a placement**, as D-84 did the sector. The
precedence is the placement's band, then the declaration's, then the baseline's
`coverage_min_elevation_rad` with the zenith. The baseline value is kept because it is the
site-wide horizon mask a deployment may know before it knows each sensor, and a baseline
that sets it keeps its meaning. It was not made a per-sensor floor to be combined with a
declared ceiling, for the reason above. `gungnir_analytics::band_or_default` is the rule's
one statement.

**Against the sensor's own vertical.** `LocalFrame::vertical_at` is the frame direction of
a 100 m step straight up from the sensor, taken from the frame's own conversion as
`true_north_at` is, so it is exactly the frame the coverage is computed in.
`CoverageVolume` carries it and `covers` measures elevation against it and bearing in the
plane square to it. The alternative, correcting each limit by the tilt's component along
the line of sight, was rejected: it is an approximation that differs by bearing, and the
exact form costs one dot product. Serde defaults (`π/2` and `[0, 0, 1]`) keep every
volume serialized before this reading as it did. A vertical that is no direction covers
nothing but the sensor's own position, because reporting a point uncovered shows a gap
rather than hides one; that guarantee, like GAP-124's, sits under
`../../agentic-workflow.md`'s numerical-stability clause and is human-owned -- see
[`../../signatures.md`](../../signatures.md).

**One builder.** `gungnir_analytics::volume_in_frame` places the position, the sector and
the band where the sensor stands, and the registry's path, DN-13's candidates and PN-16's
laydown volumes all call it, so PN-11's gap report, PN-16, DN-13 and the node's coverage
answer cannot place one sensor two ways. PN-11's rings still draw the horizontal
footprint; the band changes which approach samples are counted, which is what the gap
layer draws.

**A rehearsal honours a stated band** on top of the detection model's own altitude band,
as it does the sector, before any draw, and a changed band counts as a move. A sensor
that states none is left to its model: the baseline's floor is a coverage-analysis
setting, and applying it to a rehearsal would have changed every rehearsal of every
deployment that sets one.

## GAP-158: what the tests hold

`gungnir-analytics/tests/coverage_accuracy.rs`'s frame-stated fixture states a ceiling on
five of its six volumes and recovers both limits; its geodetic fixture recovers the
baseline floor against each sensor's own vertical. A third test declares bands on four
sensors -- at the origin, 47 km east, 46 km north-west, and one declaring none -- runs
them through the registry, and recovers every floor and ceiling by probing in a frame
anchored at the sensor itself, which states that sensor's vertical and true north
independently of the volume: worst 0.055 degree. The same volume measured against the
frame's vertical misses the 47 km sensor's floor by 0.37 degree and its ceiling by 0.40,
which the test asserts, so the correction is what passes it. PN-16's path is held in
`gungnir-app/tests/planning_panel.rs` (a 5 degree ceiling leaves an approach's steep
first 750 m uncovered, a half-degree floor its far end, a declared band is inherited and a
placement's replaces it, and the baseline floor applies only where no band is stated), the
node's answer in `gungnir-node`'s own tests, validation in `gungnir-config`, and the
rehearsal gate in `gungnir-sensor-sim`.

## GAP-147: what was wrong

A US-15 session's rehearsal of round 1's laydowns re-observed nothing: under DN-32 §5.5's
frame round 1's harbour and plant sit at the recordings' origin, and no committed
recording brings a target within 25 km of it before its excerpt ends. DN-32 §10's round-1
row was tested against a raid its own test wrote and nobody else could run.

## GAP-147: what was decided (D-112), and why not the alternatives

The gap had left the choice for the owner, as round-1 content of the kind D-28 decided.
This batch of 2026-09-26 put it under the delegation; the answer adds to round 1's content
and changes none of what D-28 decided.

**A committed sample set, TT-11**, generated by both generators from the same four YAML
files: six of TT-01's propeller drones down round 1's declared approach -- its three
points converted to ENU about round 1's origin, at its declared 300 m -- onto the harbour,
with round 1's two radars as the recording's own sensors where `current` sites them. Both
generators produced it byte for byte alike on the first run, and it passes the validator's
22 checks.

**Reusing an existing set was rejected on the evidence.** No set comes within 25 km of the
origin before it ends, so reuse means moving round 1 relative to the recordings: its
origin, its laydowns, or §5.5's rule. The first two rewrite round 1's coverage story --
D-28's `c`, the numbers `rehearsal.rs` asserts, US-15's card -- and the third changes what
every rehearsal of every deployment means. `no_plan_07_recording_reaches_round_1s_radars`
holds the evidence and fails if it stops being true. Lengthening an existing excerpt until
its raid arrived was rejected too: it rewrites a set that gated rows measured.

**How the rows that count "all ten" read.** The ten are the plan-07 library, one scenario
per mission vignette, and the criteria that name them were agreed over those ten. TT-11
is a rehearsal fixture for a usability session. The tests behind those rows iterate the
committed directory, so they now run over TT-11 too, and all pass with it -- byte and
sidecar parity, the gateway replay, the whole-pipeline replay, the statistical match, and
the fuzz corpus's seeding rule once its six seeds were added -- which is more evidence
under unchanged criteria. No criterion was edited, and the round-1 row stays Draft.
`scenarios.yaml`'s version is not bumped, because nothing the ten were generated from
changed: they are byte for byte what they were. `gungnir-ml` leaves TT-11 unassigned to a
split, because it is not a library scenario.

**What a session sees.** `TestTrackNumber::ALL` is eleven, so PN-16's picker offers TT-11,
and US-15's card names it. Rehearsed over TT-11, `current` and `b` give S1 1 142
detections and S2 2 264, and `c` gives S1 the same 1 142 and S2 2 336; `c`'s row names S2.

## GAP-121: what the owner decided (D-110)

`gungnir_resilience::StoreAndForwardQueue` had no production caller, while
`ARCHITECTURE.md` §8.4 had said it carried envelopes during an outage. The owner decided on
2026-09-26 to keep it for a future use and to document it, everywhere it appears, as
unused. Its doc comment now says so and why each path that forwards across an outage has
a queue of its own: detections in `gungnir-remote`'s bounded outbox, which is this rule over
another type in a crate `ARCHITECTURE.md` §7.1 does not connect to this one; exchange sets
in the one-set-per-item outbox (D-76, DN-18 §13), where a first-in-first-out bound would
keep obsolete sets; and an outage's decisions as one batch rebuilt from the journal
(DN-31 §15), where dropping the oldest would lose a decision. `ARCHITECTURE.md` §8.3 and
§8.4, the crate's description, `gungnir-capabilities.md`'s entry, the glossary, the UAF
service registry (SV-25 is now "Reconciliation on reconnect", pointing at `reconcile`) and
its hand-written service views no longer claim the queue carries anything. The
`gungnir-resilience` row's criterion is unchanged; the note beside it says the queue is
kept unused.

## Not done here

The Coverage accuracy row's criterion cell still describes a volume without an azimuth
sector; that is the owner's cell and GAP-159's, as before. Walking the row with these tests
as its evidence is the owner's.

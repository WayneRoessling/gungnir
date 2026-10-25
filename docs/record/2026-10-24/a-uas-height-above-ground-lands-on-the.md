# A UAS height above ground lands on the DEM

GAP-199, built to D-130 (the owner's walk of 2026-10-23). An ASTERIX Category 129 record
may state a UAS's height only above the ground under it (I129/100) and leave out the height
above mean sea level (I129/090). Until now such a UAS was held at height 0 with the baseline
30 m vertical sigma, so the tracker read a height that could be hundreds of metres wrong as
a measured one.

## What was built

- **The model.** `UasAltitudeReference` gains `TerrainPlaced { terrain, ground_m,
  ground_sigma_m }`, an ellipsoidal height, and `NoAbsoluteHeight` now carries the reason
  no height was placed. `is_height_unknown` says which reports PN-03 marks.
- **The codec.** It sets `NoAbsoluteHeight` with one of two reasons: the record has
  I129/100 but the codec reaches no ground model, or the record has neither height. It
  never uses I129/100 itself, as before.
- **The lookup interface.** `gungnir_ingest::terrain` names `GroundElevation` (a ground
  height and its error at a WGS-84 latitude and longitude) and `TerrainHandle`. It is the
  same shape as `crate::geoid` (D-123), so no dependency edge is added.
- **The adapter.**
  - **Placing.** `FeedSinks::terrain` lends the model to every feed. A record with
    I129/100 and no I129/090 is placed at the ground's ellipsoidal height plus I129/100.
  - **Refusing.** It is held at 0 as unknown, with the reason, when there is no I129/100,
    no model, or no ground there. The same happens when the model answers with a
    non-finite height or an error that is not positive.
  - **The variance.** `uas_position_variance` adds the DEM's error squared to the up axis
    for a placed height, and `UNKNOWN_UAS_HEIGHT_BOUND_M` squared for an unknown one.
  - **The counts.** `uas_heights_terrain_placed` and `uas_heights_unknown` count each case.
- **The desktop.** `gungnir_app::ground::PlacedGround` reads the placed DEM's surface,
  which is the same surface PN-02 draws and line of sight masks against:
  1. The position goes into the frame.
  2. The cell under it is found from a coarse subsample of the grid, at most 65,536
     vertices, and its triangle is interpolated.
  3. The point goes back out of the frame for its ellipsoidal height.
  4. The lookup repeats once at that height, because the frame's up tilts with distance
     from the origin.

  A hole or a position off the grid is an error. `lend_to_feeds` lends the model when the
  terrain is loaded and the baseline states its error, and otherwise says why.
- **The baseline.** `terrain.vertical_sigma_m` is the DEM's vertical error as one standard
  deviation, as its producer states it. It is optional, and must be positive and finite.
  Without it nothing is placed on the DEM: a ground height of unstated accuracy would be
  weighed as exact, and a default error would be a figure nobody stated.
- **The panels.**
  - PN-03 marks such a track's U "unknown".
  - PN-04 shows the height above ground as sent, and says where it was placed.
  - PN-09 counts placed and unknown heights per feed.
  - The node's health line counts unknown heights.

## The figure D-130 left open

D-130 said the variance of an unknown height would be "chosen when this is built and
recorded with it". It is 10 km (`UNKNOWN_UAS_HEIGHT_BOUND_M`), added as its square to the
up variance, the way D-124 adds EGM2008's largest separation for an uncorrected height.

The UAS is somewhere between the lowest ground on Earth and its highest plus its own height
above ground. Its height is held at 0. The highest ground is Everest's summit, 8,848.86 m
above mean sea level (the 2020 survey), whose geoid separation is a few tens of metres. So
10 km covers the summit and over a kilometre above it. Two other choices were set aside:

- **A tighter figure taken from the deployment's own terrain.** It needs a DEM, which is
  exactly what this path lacks.
- **A uniform distribution's sigma.** A range W gives W/√12, which is smaller. It would
  claim knowledge of where in the range the UAS is, and nothing supplies that.

## What was not built

- **The node.** It still loads no terrain, as D-130 records, so every height above ground
  there is unknown.
- **A Category 129 accuracy item.** The record carries no vertical accuracy for I129/100,
  and the baseline's 30 m covers the sender's own error, as it does for every other
  cooperative height.

## Checks

- **The adapter.** `gungnir-ingest`'s tests cover each path through the adapter: no
  height, no DEM, a DEM lent after binding, and a DEM that refuses for three reasons.
- **The lookup.** `gungnir-app/src/ground.rs` checks it against a plane, which it
  reproduces exactly between vertices. It also checks the hole and off-grid refusals, and
  a read through the frame at the origin.
- **The desktop.** `gungnir-app/tests/uas_above_ground.rs` runs the whole desktop path,
  from a written DEM through the lent handle to PN-03 and PN-04. It covers the error
  stated and not stated.

## Ownership

The adapter's Category 129 arm and `gungnir-ingest/src/terrain.rs` are the
`gungnir-ingest` gateway, which is human-owned (`docs/agentic-workflow.md`), as GAP-196's
geoid arm was. `docs/signatures.md` records their signature.

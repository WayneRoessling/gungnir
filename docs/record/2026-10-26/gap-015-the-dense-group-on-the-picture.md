# GAP-015: the dense group on the picture

GAP-015's action named three remaining engineering items for the dense-group mode: the
`spawn_blocking` plumbing, a `TrackingConfig` vocabulary, and a consumer, since nothing
projected `PipelineSnapshot::dense_group` and no picture showed a group estimate. D-46's
build (`../2026-10-25/d-46-the-dense-group-filter-is-chosen.md`) gave the mode its
vocabulary. This item records the consumer.

## What was built

The estimate now follows the same path to the screen that retained bearings already take
(GAP-096), at every layer, so an embedded desktop and a node-connected one see the same
group:

- **`gungnir-model`.** `DenseGroupView` and `GroupComponentView` are the third thing the
  picture can hold, beside a track (a place with an identity) and a bearing ray (a
  direction with no place): a count and a shape with no identity.
  - The view carries the epoch, the expected count, the CPHD's most probable count and
    count distribution, the filter, the scan size, and a stale flag.
  - Each component carries its expected count, position, velocity and one-sigma extent.
  - **Neither type has an identifier field**, so nothing downstream can draw or count a
    component as a track.
- **`gungnir-tracking-service`.**
  - `TrackingService::dense_group` defaults to `None`, as `bearing_rays` defaults to empty.
  - `project_dense_group` carries the estimate across arithmetically.
  - `LiveTrackingService` replaces the group on every snapshot, `None` included, so a
    released mode leaves no old raid behind.
  - It also ages the group by the one staleness rule tracks follow (GAP-012), so a feed
    that goes quiet cannot leave a raid on the picture reading as current.
- **`gungnir-api`.** The v3 `SnapshotResponse` gains a defaulted `dense_group`. A partner
  caller receives none of it and is told it was withheld, because like the retained
  bearings it is this deployment's internal sensor picture, and DN-18 names no exchange
  item for it.
- **`gungnir-node`** publishes `tracking.dense_group()` with each picture.
- **`gungnir-remote`.** The link's projection and `RemoteTrackingService` carry the group.
  It refreshes per snapshot, the cadence the bearings already have, because no envelope
  carries either between snapshots. The label says which epoch the group is of.
- **`gungnir-app`.**
  - The failover wrapper delegates the group rather than dropping it, the defect the
    wrapper once had for bearings.
  - `dense_group::tick` raises one alert on PN-08 when a group appears, saying the tracks
    shown are not the whole picture, and one when it goes.
- **`gungnir-viewport3d`.** `LayerInputs` gains `dense_group`, and `draw_dense_group_2d`
  draws it in both renderers.
  - Each component holding at least half a target gets a translucent two-sigma disc. That
    threshold is the PHD's own conventional extraction threshold, so the hundreds of slivers
    in the mixture do not paint the raid's whole neighbourhood.
  - The discs use the unknown-identity colour, because a group has no identity.
  - One label sits at the group's weighted centre. It reads "dense group, not tracks", the
    expected count as a mean, the filter, the CPHD's most likely count, the epoch, and
    "stale" when it is.
  - There is no layer toggle, for the reason predicted tracks have none: a raid the
    operator cannot see is the failure GAP-015 is about.

`gungnir-fusion-async`'s doc comment on `PipelineSnapshot::dense_group` said nothing read
the field; it now names the consumer. That one-paragraph doc correction is the only change
to a human-owned crate, and its standing is in `../../signatures.md`.

## How it was tested

- **The facade.** A scan of eight targets past a threshold of four puts a group on
  `dense_group()`, stamped with its epoch rather than the poll time, from the default PHD,
  with no most probable count. With nothing arriving, the host's clock running past the
  staleness limit marks it stale. The projection test checks that position, velocity, the
  one-sigma extents, the CPHD's count and distribution, and the filter name carry across
  exactly.
- **The link.** A node publishing a group is read back field for field by a desktop
  connected over the real v3 transport, beside a track it does not add to.
- **The wire.**
  - The fully populated snapshot round trip now carries a group.
  - The model's every-view round trip covers both new types.
  - A partner's snapshot holds no group and counts it as withheld.
- **The screen.** A rendered viewport with a group says "dense group, not tracks", the
  count, the filter and the most likely count, and its status line still counts zero
  tracks. The label test checks the PHD/CPHD and stale variants, and the alert test checks
  the alert line.

All suites of `gungnir-model`, `gungnir-tracking-service`, `gungnir-api`, `gungnir-remote`
and `gungnir-viewport3d` pass. So do `gungnir-app`'s unit tests and its four integration
tests whose call sites changed, and `gungnir-node`'s two. Clippy reports nothing new.

## What remains of GAP-015

- **The `spawn_blocking` plumbing**, which the mode needs before its default could be
  switched on. It changes the concurrency shape of a human-owned crate for every
  deployment.
- **A live update for a connected desktop between snapshots.** That would need an
  envelope variant for the group, as the bearings would.
- **The labelled filters** that would carry identity remain refused by name, as before.

# GAP-206 tracks releasable from configuration

Plan 12 tranche A, the fourth build. The review found that no production track was ever
releasable. Both projections stamped `Releasability::Internal` on every track, no baseline
field could set anything else, and the exchange gate withholds `Internal`. A track crossed a
peer link or an exchange agreement only in tests that built it as `AllPeers`, so the
two-node picture the architecture describes had no producer. GAP-206 sits in tranche A
because every exchange item in tranche C depends on it.

## The owner's four choices, 2026-10-27

Each was the recommended default.

| Question | Chosen |
|---|---|
| How is a track's marking derived? | Per track, from the sources that fed it. This touches the human-owned `gungnir-fusion-async` |
| What does a peer-sourced input carry? | `Internal`, fixed. Validation refuses a peer row saying anything else, and names GAP-218 |
| Operator raise and lower? | Not in this gap. The configured markings only |
| Where must a named party appear? | In the peer table, the endpoint table, or an exchange agreement's party |

## What was built

- **`gungnir-fusion-async` (human-owned).** The pipeline records every source id that moved
  each track: the detection that started it, each one associated to it, and each bearing
  that refined it. `TimedTrack` carries them as `sources`, ascending. The record is removed
  with the track. The association now keeps each measurement's source id beside it, and
  nothing else about association changed.
- **`gungnir-model`.** `SourceMarkings` maps a source id to its marking, with a deployment
  default, and marks a product by `Releasability::combine` over its sources. Its default
  marks everything `Internal`.
- **`gungnir-tracking-service`.** `with_source_markings` takes the map. Each projected track
  carries its own sources as `provenance.source_sensor_ids`, which was empty on every track
  before, and its marking is combined from them. `project_track` takes the marking as an
  argument instead of stamping the default.
- **`gungnir-config`.** A `releasability` section with `default`, `sensors` and `peers`
  rows, and `ConfigBaseline::source_markings`, which fixes every peer's `source_id` at
  `Internal`. The validation follows DN-17 §6. A sensor row must name a configured sensor,
  once. A named party must be known to the baseline. An empty party set is refused. A
  peer row may only say `internal`. A misspelt key is refused. The section applies as a
  Deployment section, beside the exchange agreements it is the second gate of.
- **PN-04 names the sensors.** The track detail card already drew the marking and an
  origin line beside it. With no sources on any track, that line read "From no named
  sensor" for every local track. It now reads, for example, "From sensor(s) 1, 2".
- **Both binaries** hand the map to their tracker: the node in
  `picture::tracking_service`, in all three of its arms, and the desktop in
  `configured_tracker`.
- **Documents.** DN-17 gains §9, amendment 1, which also explains the one-section shape.
  Its status line, which still read "design only", is corrected. The deployment README
  gains "Releasing tracks to a partner". The roles table names the section with the
  exchange agreements.

## Why one section rather than a field on each row

DN-17 §6 gives the baseline a deployment default "plus a per-sensor and per-peer
override". A field on `SensorConfig` and `PeerConfig` would have said the same thing. A
release authority, though, reviews a distribution rule as one table, and a rule spread
across the sensor list is one nobody reads whole. The section keys sensors by id and peers
by name.

## Why the marking is not carried on each detection

The gap's action said the marking would be carried on `Provenance` into the projection.
The pipeline consumes a detection's sensor id and position, not its provenance. So a
marking stamped on each detection would stop at the pipeline's door. The track instead
records its sources, and the marking is computed from those at projection, against the
baseline's map. The provenance still carries the result: each track's own sources, which
it never did before.

## Tests

- **`gungnir-node/tests/releasable_tracks.rs`, the two-node test.** Node A runs its own
  tracker, gateway and picture publication. Its baseline releases radar 1 to node B by name
  and leaves radar 2 at the default. Node B links to it as a machine over mutual TLS, under
  its own pinned peer identity. Exactly radar 1's track crosses, and the partner is told
  its picture is partial. Then node B's half: the track it heard is a peer source, and it
  stays `Internal` on B under B's all-peers default, so it cannot go back to A.
- **`gungnir-fusion-async`.** Each track records the sources that fed it, and a deleted
  track's record goes with it.
- **`gungnir-tracking-service`.** Each track is marked by its own sources, and a service
  given no markings releases nothing.
- **`gungnir-model`.** `SourceMarkings` marks by the most restrictive source, falls back to
  the default, and is `Internal` for no sources.
- **`gungnir-config`.** The section parses as documented and is `internal` when absent.
  Each refusal is shown against an otherwise valid baseline, and a peer stays `Internal`
  under an all-peers default.

## What this does not do

- **No operator raise or lower.** The `product.release` action DN-17 §6 names for changing
  a marking is not built here.
- **A peer's tracks are never released again.** That waits for GAP-218's re-ingest guard.
- **Exchange parties add no name today.** Validation already requires every exchange
  party to be a peer or an endpoint. The third clause keeps the rule true if that ever
  widens.
- **The dense-group estimate and the bearing rays are unmarked.** They stay withheld from
  every partner, as before, because DN-18 names no exchange item for either.
- **Human-owned standing.** `gungnir-fusion-async` is human-owned. What the owner has
  signed of this change is in `../../signatures.md`.

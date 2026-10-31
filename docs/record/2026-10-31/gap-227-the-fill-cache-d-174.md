# GAP-227: the fill cache (D-174)

The last part of `docs/design/DN-36-staged-terrain-and-map-tiles.md`: D-160's fill when
connected, on D-173's client, in the form the owner chose for D-174 on 2026-10-31. That form
is a URL template for one-degree geographic tiles named `{ns}{lat}{ew}{lon}`, as USGS 3DEP
names them, over a mirror store's URL or an OGC Web Coverage Service. DN-36 amendment 6
records the build.

**The baseline.** `terrain.fill_source` is optional, skipped when absent so signed baselines
still serialize unchanged. Validation refuses it without a `store`, without all four
placeholders, and over anything but `https://` or `http://` to the loopback.

**The data half, `gungnir_data::terrain_store::fill`.**
- `OneDegree` names a tile and expands the template.
- `covering` lists the tiles a box of degrees touches.
- `open_cached` reopens a sub-store against the digest recorded in `<name>.fetched.json`.
- `fetch_and_stage` fetches the tile into the cache directory, stages it with increment 1's
  `stage` in the pinned store's frame, vertical system, realisation and tile size, records
  the URL, the source digest, the manifest digest and the time, and removes the download.
- The fetch moved here from the command's module; the command and the fill share it and its
  URL rule, `cli::check_url`.

**The desktop, `gungnir_app::terrain_tiles`.**
- The tile worker reads the pinned store's coarsest tile once, to learn whether the store is
  geographic.
- When the view reaches past the edge, the worker opens or fetches the one-degree tiles the
  view touches outside the store, one at a time and at most four per view, then places their
  footprints and the tiles the view wants of them.
- No tile is asked for twice in a session.
- What it is doing, or why it failed, goes to the map and PN-09. PN-09 counts fetched tiles
  apart from pinned ones.

**The viewport.** `TerrainStoreLayer` carries `fills` and a `fill_status`. `TerrainTiles`
chooses fill tiles within what the pinned tiles leave of the frame's budget
(`tiles_in_view_within`). It leaves out a fill triangle wherever its centre lies inside a
level-0 tile the pinned store holds, joins the rest into the one surface, and outlines each
fill tile drawn. `StoreFootprint::frame_of` maps the view back to degrees through the
store's placed corners. The surface cache is now keyed by store as well as tile.

**Line of sight is untouched.** The worker's field reads the pinned store alone, and
`ground_past_the_edge_is_fetched_marked_and_kept_out_of_line_of_sight` holds it bit for bit
against the same baseline with no fill source (§8.1 row k). The same test checks that
nothing is fetched with no fill source (row h's first half). It checks that the fill is
outlined, said and counted apart (row j), and that the map's missing-ground sentence gives
way over fetched ground (§8.2's added check). It checks that the cache records the URL, and
that after the session every pinned file still matches its manifest, the manifest still
matches the baseline, and no file was added to the pinned store (row i).

`an_unreachable_fill_source_is_said_and_a_cached_tile_is_reused` points the template at a
closed loopback port. It finds the failure said on the map and PN-09, with the
missing-ground sentence still shown (row h's second half). It then shows that a tile fetched
in an earlier session is reopened from the cache with no further request. Every fetch in
these tests is from a mirror on the loopback, which serves a 50 by 50 tile written by
`geotiff::write`.

**CI.** `ci.yml`'s test job runs the `terrain-fetch` tests in its first shard, since the
workspace run never compiles the feature.

**Not done, and not this note's.** The non-geographic refusal is not exercised by a test.
The node draws nothing, so it fetches nothing. GAP-227 stays open for D-139's symbols, the
cursor readout and the declutter (DN-37).

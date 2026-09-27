# A converted height carries its vertical datum

GAP-108, GAP-196 and GAP-197
([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml));
D-121 and D-122
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml));
[`../../rust-3d-data-ecosystem-build-vs-adopt.md`](../../rust-3d-data-ecosystem-build-vs-adopt.md)
§5, amendment 1. Built on 2026-09-26. No human-owned crate is touched: `gungnir-data`,
`gungnir-config`, `gungnir-app` and `gungnir-ui` are all outside the list in
[`../../agentic-workflow.md`](../../agentic-workflow.md).

## What was wrong

A DEM or point cloud converted out of a real-world CRS kept its height in the file's own
vertical datum. The height was scaled to metres and nothing more. The picture reads every
altitude as a WGS-84 ellipsoidal height, so a converted surface sat off by the geoid
separation at that place. That is -22.6 m at the Autzen fixture in Oregon and +34.9 m at
the Baltic fixtures: a steady offset, never noise, and never said.

GAP-102 recorded this and D-51 gave it a row. A second defect sat under it, which GAP-023
had recorded on 2026-09-08 and left alone. `TerrainMesh::with_xy` placed a converted
DEM's east and north and carried the file's own height through as its up. So even a
correct ellipsoidal height would have been drawn at the wrong up: off by the origin's own
altitude, and by the Earth's curvature away from the origin (about 31 m at 20 km). Line
of sight was masked against that surface.

## What was decided, and by whom

**The owner decided on 2026-09-26 (D-121).** NGA's EGM2008 2.5-arc-minute geoid grid,
public domain, is applied through PROJ behind the existing `crs` feature, giving
centimetre-level vertical accuracy. A file whose vertical datum has no grid is refused by
name, never a silent ellipsoid/geoid mix.

## What was taken under the delegation, and why

**Distribution.** The grid is PROJ-data's GeoTIFF, `us_nga_egm08_25.tif`, from
`cdn.proj.org`, 80 585 622 bytes. It is pinned by the SHA-256 that PROJ-data publishes for
it in its own index (`files.geojson`): `4191d471...f17a`. The copy downloaded for this
work matched that digest. The digest is in `deploy/geoid/SHA256SUMS` and in
`gungnir_data::geoid::EGM2008_GRID_SHA256`, and a test holds the two equal.

- The file is never committed. GitHub refuses files over 100 MB, and 80 MB of binary has
  no place in the history either way.
- A deployment names the grid's directory in `geoid_grid_dir`; without it the desktop
  looks in `PROJ_DATA`. `deploy/README.md` gives the install step for both shells, and
  the disconnected case: fetch and check it on a connected machine, then carry it across.
- Rejected: committing the grid, or Git LFS. The first is the size problem above. The
  second makes every clone depend on an LFS endpoint, for a file most deployments never
  need.

**Only a verified file reaches PROJ, and PROJ cannot fall back.** A `GeoidGrid` can only
be made by hashing a file and matching the digest. The conversion takes a `GeoidGrid`,
so an unverified file never reaches it. Two things in PROJ would otherwise defeat that:

- **PROJ searches for a grid by name** along `PROJ_DATA`, and the first match wins. So a
  stale copy earlier on the path would be read instead of the file that was hashed. PROJ
  is therefore handed the verified file's **absolute path**, double-quoted, so a
  `Program Files` path still works. A path holding a comma or a double quote is refused
  first, since PROJ's grid list would split or end on either.
- **PROJ's own operation search falls back.** Asked for "EGM2008 height to EPSG:4979",
  PROJ offers `vgridshift` with this grid and, when the grid is missing, a "ballpark"
  no-op that returns the height unchanged. `pyproj` lists both: the second is "without
  ellipsoid height to vertical height correction". That no-op is exactly the silent mix
  D-121 refuses. So the operation is written out in full, and a grid that disappears
  after verification is a refusal from PROJ, never a zero. A test removes the file after
  verifying it and checks the refusal.
- **`proj` 0.31's `Proj::convert` zeroes `z`** and returns only `x` and `y`, which GAP-102
  already recorded. Fed a zero height, `vgridshift`'s output height *is* the undulation
  N. An `axisswap order=1,3,2` step moves it into the second slot, which `convert`
  returns. PROJ does all of the geodesy; the swap only routes its answer past the binding.
  The alternative was calling `proj_trans` through `proj-sys` directly. That is `unsafe`
  FFI, human-owned, for a result the safe API already gives.

**libproj reads GeoTIFF grids.** `proj`'s `tiff` feature does not exist on its own; only
its `network` feature turns `tiff` on, and `network` is run-time grid fetching, which
stays refused. So `proj-sys` is named directly in `[workspace.dependencies]` for its
`tiff` feature alone. That builds libproj with `ENABLE_TIFF` and links a system libtiff.
`proj-sys`'s from-source build keeps `ENABLE_CURL` off whatever features are set.

- It is the 0.27 that `proj` already resolves, so no crate enters the graph.
- No workspace code calls it.
- `cargo tree -d` on the release targets is unchanged, since neither builds `crs`.
- §2.9 carries its row.

**What a height's datum is.** The rule lives in `gungnir_data::geoid::height_reference`.

- **The file states its datum where it can:** a WKT `VERT_CS`, read by its `VERT_DATUM`
  (type 2002 is ellipsoidal; EPSG:1027 is the EGM2008 geoid) and then its own authority;
  or a `GeoTIFF` `VerticalGeoKey`.
- **A file that states none** takes the baseline's new `terrain.vertical` or
  `point_cloud.vertical`, `"ellipsoidal"` or `"epsg:<code>"`. A plain UTM WKT or a DEM
  with no vertical key is the ordinary case.
- **A baseline that contradicts the file is refused**: the file's tags are what its
  numbers are, the rule the horizontal frame already follows.
- **What converts:** ellipsoidal heights pass unchanged. EGM2008 heights gain N.
- **What is refused by name:** every other datum, and a height nobody states. The refusal
  names the field to declare, or the datum and that no grid ships for it.
- Rejected: reading an unstated datum as ellipsoidal, or as EGM2008. Either is the guess
  the decision ends. Rejected too: approximating EGM96 or NAVD88 by EGM2008. EGM96
  differs from EGM2008 by decimetres to metres, and NAVD88 by about a metre in the
  Pacific Northwest, so that is a smaller silent mix and still one.

**The Autzen capture's NAVD88 heights are now refused.** Until now that capture was
converted and drawn, its heights about 22.6 m high. It is now refused through a real tick,
and the refusal names "NAVD88 height (ftUS) (EPSG:6360)". This is a behaviour change for
anyone who had configured that file with an `epsg` frame. GAP-197 records what the
refusal costs, and what would lift it: more grids on the same pinned mechanism.

**A DEM in feet is refused at load.** Reading a file's `VerticalGeoKey` raised the
question of its `VerticalUnitsGeoKey` (4099). The DEM path has always read heights as
metres, and a height in feet read as metres is a silent factor of 3.28, which a geoid
correction would only hide. A key naming any unit but the metre is now refused by name.
GDAL omits the key for a metric system. For a coded system such as EPSG:6360 GDAL omits
it too, since the code implies the unit. Every datum that converts (3855, 4979, 5030) is
metric by definition, so a code-implied foot can only arrive with a datum that is
refused anyway.

**D-122: a converted DEM's vertex lands at its local up.** The up comes from the same
`gungnir_coord` transform that places its east and north, computed from the ellipsoidal
height. This is `TerrainMesh::with_enu`, which replaces `with_xy`. A no-data vertex stays
a hole.

- Rejected: leaving the file's height as z. That is the defect GAP-023 recorded.
- Rejected: putting the ellipsoidal height in z. That is right at the origin only.
- A `"local-enu"` DEM is untouched: its heights were prepared as the deployment's own up.

**The desktop's start-up check, and PN-09.** On the first tick the desktop finds the grid
through `geoid_grid_dir`, else `PROJ_DATA` entry by entry.

- It hashes the file on a thread of its own. A missing file settles on that same tick,
  so a deployment without a grid starts its terrain on the first frame like any other.
- The terrain and the point-cloud pair wait until the check settles. So an EGM2008 file
  never meets a grid that is still being hashed. That costs a fifth of a second once,
  in a release build.
- PN-09 gains a geoid line: verified (with the path and where it was named), refused
  (with the reason, and an alert), or none installed.
- The line also says when the build has no `crs` feature at all. The Windows desktop
  `release.yml` publishes has none, and D-51 keeps the feature off by default.
- The node converts no terrain or point cloud, so its image needs no grid.

**CI fetches the pinned grid, and that is decided rather than assumed.** The workflows
set no policy against network fetches: `rust-cache`, `apt-get` and the actions themselves
all reach the network. What makes a fetch acceptable is the pin, and this one has it:

- `sha256sum -c` against `deploy/geoid/SHA256SUMS` runs before any test reads the file,
  on a cache hit as much as a miss.
- The file is cached under that digest and saved from main only, the cache-quota rule
  the Rust cache already follows. So the CDN is asked once per eviction.
- The clip-based tests run first and need no network at all.
- The anti-vacuous gate counts the `#[ignore]`d full-grid run separately, so a missed
  `--ignored` fails the job instead of passing as zero tests.

## How it was checked

**The clip.** `testdata/geoid/egm08_25_clip_53n56n_13e17e.tif`: 97 by 73 nodes, 15.9 kB,
around the DEM fixtures and `five-points.las` read as EPSG:32633. It was cut with GDAL
3.11.3 (the OSGeo image) by node offsets from the pinned file. `SOURCE.md` records the
command and its digest.

**The independent check** (the GAP-102 precedent: `pyproj`, recorded and not committed).
`pyproj` 3.8.0, bundling PROJ 9.8.1, with the network off. That is a different PROJ from
the 9.6.2 `proj-sys` builds.

- At five points, one on a node, it gave the same undulation three ways, to 3e-14 m:
  with PROJ choosing its own operation over the full grid; with the written-out pipeline
  over the full grid; and with the same pipeline over the clip.
- A hand bilinear interpolation of the clip's nodes, as GDAL's XYZ export prints them,
  agreed to 3e-11 m. That residue is the ten-decimal printing.
- `Transformer.from_crs(<the WKT GDAL writes for EPSG:32633+3855>, "EPSG:4979")` gave the
  latitude, longitude and ellipsoidal height of `five-points.las`'s first and fifth
  points.
- The expected local ENU of the app-level tests was computed with `pyproj` for the
  horizontal, the geoid and geodetic-to-ECEF, and a hand rotation into the origin's
  tangent plane. Nothing of `gungnir_coord`'s went into it.

Every value is transcribed into the tests, which state their tolerances: a micrometre for
an undulation, 1e-9 degrees and a micrometre for the point conversion, and a tenth of a
millimetre for a placed up (the `f32` step).

**Run before pushing, on Linux.** The Windows host cannot build `proj-sys`, so the tests
were run in `rust:1.98-slim-bookworm` with `cmake`, `libsqlite3-dev`, `sqlite3` and
`libtiff-dev`, the job's own set. That covers `cargo test -p gungnir-data --features crs`
with the full pinned grid, `cargo test -p gungnir-app --features crs --test terrain --test
pointcloud_crs --include-ignored`, and clippy with the feature on. PROJ 9.6.2 built from
source with its TIFF reader, and every test passed, the desktop's own start-up check
against the 80 MB grid included.

**Negative check.** With the undulation not added (`egm2008_to_ellipsoidal` returning the
heights unchanged), four tests failed: the undulation on the clip, the point-cloud
conversion, the DEM placed through the clip, and the pair placed through a real tick.
The old file-height-as-up misses the existing UTM test's new up assertion by nine times
its tolerance.

## What is not done

- **GAP-196.** A cooperative UAS's altitude above mean sea level (ASTERIX Category 129,
  I129/090) is still placed as an ellipsoidal height, with the loss recorded on the
  report. Whether the grid now corrects it depends on which geoid a sender's "mean sea
  level" is. `gungnir-interop` reaches no grid in any case.
- **GAP-197.** Every datum but EGM2008 is refused, which leaves NAVD88 LIDAR and
  EGM96-based DEMs unusable until more grids are admitted the same way.
- **A LAS 1.0-1.3 file's geokey vertical system and unit** stay unread, as GAP-102
  recorded. Such a file is refused before its datum is asked.
- **The horizontal datum step for NAD83** is still the null transformation PROJ selects
  for a 2D NAD83 to WGS 84 conversion, good to a metre or two. An ellipsoidal height in
  NAD83 is taken as WGS-84's to the same accuracy. That is the horizontal conversion's
  existing accuracy, not a vertical-datum question, and it is stated here so nobody
  reads centimetres into it.

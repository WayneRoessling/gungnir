# NAVD88 and EGM96 heights convert, and a LAS geokey file states its heights

GAP-197, GAP-102 and GAP-023, and GAP-200 filed
([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml));
D-125 and D-126
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
both taken under the owner's delegation of 2026-09-26;
[`../../rust-3d-data-ecosystem-build-vs-adopt.md`](../../rust-3d-data-ecosystem-build-vs-adopt.md)
§5, amendment 2. Built on 2026-09-26. No human-owned crate is touched: `gungnir-data`,
`gungnir-config`, `gungnir-app` and `gungnir-ui` are all outside the list in
[`../../agentic-workflow.md`](../../agentic-workflow.md).

## What was wrong

GAP-108 (D-121) pinned one geoid grid, EGM2008, and refused every other vertical datum by
name. That ended the silent ellipsoid/geoid mix, and left most United States LIDAR
(NAVD88) and EGM96 DEMs such as SRTM unusable. The Autzen capture, the fixture GAP-102
was built around, had been refused since then.

GAP-102 had also left a LAS 1.0-1.3 file's `VerticalUnitsGeoKey` unread, so every such
file was refused before its datum was asked, and checked a baseline's EPSG code against a
WKT only for containment, which the NAD83 datum's own code passed.

## What was decided, and why

**D-125: two more grids, admitted exactly as EGM2008 was.**

- **EGM96**: NGA's 15-arc-minute grid, PROJ-data's `us_nga_egm96_15.tif`, 2 710 815
  bytes, SHA-256 `db493027...0e78d`, for EGM96 heights (EPSG:5773).
- **GEOID18**: NOAA's conterminous United States grid, PROJ-data's
  `us_noaa_g2018u0.tif`, 16 742 155 bytes, SHA-256 `fa9a407a...0abce2`, for NAVD88
  heights (EPSG:5703, 6360 and 8228).
- Both were read from PROJ-data's own index, `cdn.proj.org/files.geojson`, and the
  downloads matched its digests. Neither is committed. Each is verified at start, gets
  its own PN-09 line, and is fetched, checked and cached by `proj-crs`.
- **Only CONUS for GEOID18, because that is all PROJ-data has.** The index holds no
  GEOID18 file for Alaska or Hawaii; their NAVD88 grids there are GEOID12B's. Its
  `us_noaa_g2018p0.tif` relates Puerto Rico and the Virgin Islands to PRVD02, not NAVD88.
  A NAVD88 point outside the CONUS grid gets `inf` from PROJ and is refused, and the
  refusal says why.
- **A datum takes its own grid and no other.** EGM96 and EGM2008 differ by 0.39 m to
  0.68 m over the Baltic fixtures; using one for the other is the smaller silent mix
  D-121 refused.

**What a NAVD88 height is worth.** GEOID18 turns a NAVD88 height into a **NAD83(2011)**
ellipsoidal height, not a WGS-84 one.

- The desktop reads NAD83(2011) as WGS 84 with no frame step. That is the same null step
  its horizontal conversion already takes for a NAD83 source.
- It is also what PROJ chooses by itself. Asked for the Autzen WKT to EPSG:4979 with
  GEOID18 on its path, PROJ picked "Inverse of Oregon GIC Lambert + NAD83 to WGS 84 (1) +
  ... + Inverse of NAD83(2011) to NAVD88 height (3) + NAD83(2011) to WGS 84 (1)". That is
  the Lambert inverse, the US survey foot, `vgridshift` on GEOID18, and null frame steps:
  this workspace's chain exactly.
- The frames differ by 1 to 2 m horizontally and up to a metre vertically across the
  United States. At Autzen, `pyproj`'s "Inverse of ITRF2014 to NAD83(2011) (1)" at epoch
  2010.0 moves a point 1.4 m horizontally and -0.38 m vertically.
- So a NAVD88 surface lands to the metre in WGS-84 terms. That is tens of metres better
  than refusing it or reading it as ellipsoidal, and it is stated wherever the conversion
  is: in the code, in `deploy/README.md`, in D-125 and in GAP-200.
- Rejected: modelling the time-dependent step now. It needs an observation epoch that LAS
  and GeoTIFF files do not carry, and applying it to the height alone would leave the
  horizontal on the null step, inconsistent. GAP-200 keeps it in the register.

**Every other datum stays refused by name**, and the refusal names the way out:
`deploy/README.md`, "Pinning a further geoid grid". That procedure is the one followed
here: find the grid in PROJ-data's index, check its type and target code, pin its digest
in the manifest and in `GeoidModel` together, clip it for tests against `pyproj`, and let
`proj-crs` fetch it.

**The desktop's grid check, for three grids.** Each grid is looked for on its own,
through `geoid_grid_dir` else `PROJ_DATA`, hashed on its own thread, and given its own
PN-09 line.

- A `geoid_grid_dir` holding none of the three is taken for a mistake: every grid is
  refused as missing, with one alert naming the directory.
- One holding some of them has simply not installed the rest, and PN-09 says so quietly.
  A European deployment is not alerted about GEOID18 at every start.

**D-126: what a geokey file's keys say about its heights.** Three writers were read
rather than guessed at:

- GDAL 3.11.3, for `-a_srs EPSG:2992+6360`, writes a GeoTIFF 1.1 directory with
  `VerticalGeoKey` 6360 and **no** unit key. The code fixes the unit.
- GDAL, for a horizontal system alone, writes a GeoTIFF 1.0 directory with
  `ProjLinearUnitsGeoKey` (9002 for 2992, 9001 for a UTM zone).
- LAStools writes `VerticalUnitsGeoKey` beside `VerticalGeoKey` (4099 = 9001 beside
  NN2000, in a Norwegian capture the `las` crate ships).

So the datum is the `VerticalGeoKey` code. The unit is `VerticalUnitsGeoKey`, else the
unit the vertical code fixes, else, with no vertical key, the projected system's
`ProjLinearUnitsGeoKey`. The last is the rule the WKT path already had. Only the metre and
the two feet are read; a contradiction or any other unit is refused by name.

- A point cloud is scaled by that unit.
- A DEM stays metres-only, GAP-108's rule, which now also refuses a `VerticalGeoKey`
  whose code fixes a foot (6360, 8228). Without that, NAVD88 converting would have made a
  GDAL-written DEM in feet a silent factor of 3.28. Rejected: scaling a DEM in feet, with
  no real DEM in feet to test it on.
- A WKT `VERT_CS` whose axis points down is a depth, and is refused by name. A
  `VERT_DATUM` authority (1027, 5171, 5103) names its datum whatever the unit.
- The baseline's horizontal code must now equal the `AUTHORITY` of the WKT's own
  `PROJCS` or `GEOGCS`. Autzen's 2992 passes. Its datum's 6269, its geographic system's
  4269, its unit's 9002 and its vertical system's 6360 all passed the old containment
  check, and are refused now.

## How it was checked

**The clips.** Each was cut with GDAL 3.11.3 from its pinned file by node offsets, and is
recorded in `testdata/geoid/SOURCE.md`:

- EGM96: 17 by 13 nodes over the same 53-56 N, 13-17 E as the EGM2008 clip, 1.4 kB.
- GEOID18: 61 by 61 nodes around Autzen, 43.5-44.5 N, 123.5-122.5 W, 9.2 kB.

**The independent check** (the GAP-102 and GAP-108 precedent: `pyproj`, recorded and not
committed). `pyproj` 3.8.0 bundling PROJ 9.8.1, with the network off and the full pinned
grids on its path.

- For each model it gave the same undulation three ways, to 7e-14 m: with PROJ choosing
  its own operation ("Inverse of WGS 84 to EGM96 height (1)", "Inverse of NAD83(2011) to
  NAVD88 height (3)"); with the written-out pipeline on the full file; and with the same
  pipeline on the clip.
- It converted the Autzen capture's WKT and `EPSG:2992+6360`, for the query-box centre,
  three points of the box in the loader's own order, and the five points of the new LAS
  fixture. It converted `EPSG:32633+5773` for the Baltic fixtures.
- The expected local ENU was `pyproj`'s ECEF and a hand rotation into the origin's
  tangent plane, nothing of `gungnir_coord`'s. The same rotation reproduces GAP-108's
  recorded figure to every printed digit.

**The new fixture.** `testdata/pointcloud/autzen-geokeys.las` is a LAS 1.2 file of five
points in Autzen's own system. Its key directory is the one GDAL wrote for
EPSG:2992+6360, copied byte for byte. So the unit and datum the loader reads come from a
layout a real writer chose (`testdata/pointcloud/SOURCE.md`).

**Run before pushing, on Linux.** The Windows host still cannot build `proj-sys`. The
feature-gated tests ran in `rust:1.98-slim-bookworm` with the `proj-crs` job's packages:
`cargo test -p gungnir-data -p gungnir-app --features crs` for `pointcloud_crs`,
`terrain`, `geoid`, `dem` and the library, and the `--ignored` full-grid tests against
all three pinned grids, the desktop's own start-up check included. Every test passed.

**Negative check.** With the undulation not added, eleven tests failed, across all three
models: the point-cloud conversions through each grid, both DEM ups, and the Autzen and
geokey pairs through a real tick. The EGM96 DEM test installs the EGM2008 grid too, so
EGM2008 standing in for EGM96 would miss it by 0.387 m, 3870 times its tolerance.

## What is not done

- **GAP-200.** A NAD83 file, and every NAVD88 height, lands at the metre level in WGS-84
  terms, as above.
- **NAVD88 in Alaska and Hawaii**, and every national datum, is refused until its grid is
  pinned by the procedure above.
- **The Windows desktop `release.yml` publishes has no `crs` feature** (D-51), so it
  converts no real-world CRS at all, and PN-09 says so.

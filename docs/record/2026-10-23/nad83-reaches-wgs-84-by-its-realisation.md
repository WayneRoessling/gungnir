# NAD83 reaches WGS 84 by its realisation

GAP-200, built to D-127 and D-128 (the owner's walk of 2026-10-23). Until now a NAD83 DEM
or point cloud was converted with PROJ's null "NAD83(2011) to WGS 84" step, which left it
one to two metres off horizontally and up to a metre vertically. D-125 accepted that and
said so; this removes it.

## What was built

- **`gungnir_data::nad83`**, a new module:
  - **Reading the datum.** A file's horizontal datum is read from PROJ's own WKT1
    description of its system. That gives NAD83(2011), NSRS2007, FBN, HARN, CORS96, the
    unqualified NAD83 (datum 6269), or not NAD83 at all.
  - **D-128's rule (`resolve`).** The file wins. An unqualified file takes the baseline's
    declaration and is otherwise refused by name. A contradiction is refused, and so is a
    declaration on a file that is not NAD83.
  - **The frame step (`FrameStep`).** The NADCON5 steps a realisation needs, over
    verified grids, then the Helmert at the epoch, as one written-out PROJ pipeline. The
    positions are first unprojected in the file's own datum.
  - **Heights (`nad83_2011_height_shift`).** The Helmert's height difference for a NAVD88
    height, which is NAD83(2011)'s once GEOID18 has run.
- **The four NADCON5 grids** for the conterminous United States are pinned like the geoid
  grids. Each is PROJ-data's file, matched to the SHA-256 its index publishes,
  `deploy/geoid/SHA256SUMS` and the constants held equal by test, never committed.
  Committed clips around Autzen serve the tests (`testdata/nadcon5/SOURCE.md`).
- **`PinnedGrid`** keys the desktop's grid checks, so the NADCON5 grids are found,
  verified and listed on PN-09 the way the geoid grids are.
- **The baseline:**
  - `terrain.nad83_realisation` and `point_cloud.nad83_realisation`, each the
    realisation's geographic code (`"epsg:6318"` for NAD83(2011)).
  - `frame_epoch`, a decimal year; absent means the current date.
- **The conversions.** Terrain and point clouds both go through the frame step:
  `pointcloud::crs::to_local_enu` takes the frame conversion, and the desktop resolves it
  from the file's datum and the declaration.

## Two findings that shaped the build

**PROJ's `helmert` cannot be given the epoch in its definition, and `proj` 0.31 cannot
pass one.** `Proj::convert` hands PROJ a time of infinity and a height of zero. PROJ 9.8.1
silently ignores a `+t_obs` parameter, which I first assumed existed. Checked with
`pyproj`, that pipeline gave the same answer at every epoch, 0.15 m off. The time-dependent
Helmert is therefore written as the static one it is at the epoch: each of the seven
parameters plus its rate times the years from 2010.0, which is how PROJ evaluates it.
Against PROJ's own time-dependent operation, over four epochs, six CONUS points, two
heights and three time coordinates, the worst difference was 0.00 m. So the geodesy stays
PROJ's, with no unsafe FFI and an exact epoch.

**A figure in the owner's walk was wrong.** The GAP-200 question said a wrong epoch costs
about a centimetre a year. Measured at Autzen it is about 2 cm a year: 0.337 m between
2010.0 and 2026.8. `docs/record/2026-10-23/the-owner-s-walk-of-2026-10-23.md` records the
correction. It argues for the epoch the owner chose, the current date.

## How it was checked

All with `pyproj` 3.8.0 (PROJ 9.8.1), network off, the full NADCON5 grids on its path:

- **The pipelines.** The pipelines this module writes agreed with PROJ's own choice of
  operations, from each realisation's geographic system to NAD83(2011) and on to ITRF2014
  at 2020.0, to within 1e-9 m. The NAVD88 height difference agreed exactly.
- **The clips** agreed with the full grids to 0.6 mm at most.
- **Autzen.** The query-box centre lands where PROJ puts it for NAD83(2011), HARN and the
  1986 adjustment, each in a different place: `gungnir-data/tests/pointcloud_crs.rs`.
- **The desktop.** Autzen, declared NAD83(2011), lands 1.485 m west, 0.286 m north and
  0.370 m lower than under the null step, through a real tick:
  `gungnir-app/tests/pointcloud_crs.rs`.

The existing tests that check the Lambert inverse and GEOID18 on their own keep doing so.
They pass a "not NAD83" frame that leaves the positions PROJ's own, and their NAVD88
heights move by exactly the Helmert's height difference at each point, about -0.3701 m at
2020.0.

## Things stated rather than hidden

- **The `crs` build was not compiled on this machine.** `proj-sys` cannot build libproj on
  Windows MSVC, and the container route needed packages not installed here. So the
  `crs`-gated code is compiled and tested by `ci.yml`'s `proj-crs` job alone, the only
  place it ever runs, and its geodesy was checked by the `pyproj` runs above.
- **GEOID18 is read at the converted WGS 84 position**, a metre or two from the
  NAD83(2011) position it is indexed by: 0.02 mm at Autzen, stated in
  `nad83::wgs84_heights`.
- **Plate-interior deformation since a survey is not modelled.** It is the residual at the
  right epoch, and every conversion states it.
- **Outside the conterminous United States** a NAD83 position needing NADCON5 is refused
  by name. PROJ-data distributes NADCON5 grids for Alaska, Puerto Rico and the Virgin
  Islands, and Guam too, and none is pinned.

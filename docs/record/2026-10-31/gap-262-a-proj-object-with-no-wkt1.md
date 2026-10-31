# GAP-262: a PROJ object with no WKT1 form is refused, not a panic

GAP-262 was filed and closed together, from what GAP-261's item
(`gap-261-every-proj-call-in-gungnir-data.md`) recorded as found and not fixed.

**The cause.** `proj` 0.31's `Proj::as_wkt` hands what `proj_as_wkt` returns to its
`_string` helper, which asserts the pointer is not null (`proj-0.31.0/src/proj.rs:183`).
PROJ returns null for an object the requested WKT version cannot describe, so asking for
one is a panic, not an error. `gungnir_data::nad83::horizontal_datum` builds a PROJ
object from whatever source it is given and asks it for WKT1, to read the datum.

**What reaches it.** A probe on the WSL Ubuntu rig (libproj 9.7.1) put sources through
`LockedProj::new` and asked each for WKT1 GDAL:

- `"EPSG:1671"`, the code of a transformation, does not: PROJ reads `EPSG:<code>` as a
  CRS and refuses it as one it cannot find. The baseline's `terrain.frame` and
  `point_cloud.frame` take only `epsg:<digits>`, so a transformation code is refused
  before the WKT call already.
- A geographic 3D CRS does. `EPSG:4979` (WGS 84) and `EPSG:7912` (ITRF2014) panicked, PROJ
  saying "WKT1 does not support Geographic 3D CRS". Either is one line in a baseline.
- So does any coordinate operation: a transformation named by URN
  (`urn:ogc:def:coordinateOperation:EPSG::1671`), and a `+proj=` string without
  `+type=crs`, which PROJ reads as a conversion. A baseline cannot name either; a file's
  own WKT reaches `horizontal_datum` whole, and one that described an operation or a 3D
  geographic system would. No file was tried.
- Every CRS that worked still does: geographic and projected 2D, compound, vertical,
  bound, and geocentric (`EPSG:4978`, three axes, written as a WKT1 `GEOCCS`).

**Where it lands.** Both callers, terrain and point-cloud placement, run in
`apply_result` inside the desktop's frame tick. Before the change,
`gungnir-app --test terrain` with `small.asc` and `terrain.frame: "epsg:4979"` panicked at
`proj.rs:183` inside `update::tick`.

**What changed.**

- `LockedProj::kind` reads the object's PROJJSON `type` and axis count. `proj`'s
  `to_projjson` checks the pointer before reading it, so this is an error where WKT is a
  panic.
- `horizontal_datum` refuses by name, before the WKT call, a source that is not a CRS
  ("is a Transformation, not a coordinate reference system") and a geographic or
  projected CRS with three axes ("is a 3D GeographicCRS, which PROJ cannot describe as
  WKT1").
- `LockedProj::as_wkt` catches the assertion with `std::panic::catch_unwind` and returns
  a `ProjError`, for any other object PROJ declines. PROJ has returned before the
  assertion fires and nothing on the Rust side is part-written, so the object is used as
  before afterwards. The panic hook still prints the assertion, which is why
  `horizontal_datum` checks first.

**Not taken.** PROJ can write a geographic 3D CRS as a WKT1 compound with
`ALLOW_ELLIPSOIDAL_HEIGHT_AS_VERTICAL_CRS=YES`, but `proj` 0.31's `WktOptions` has private
fields and no constructor, and calling `proj_as_wkt` through `proj-sys` is `unsafe` and
human-owned. Reading the datum from PROJJSON instead of WKT1 would accept a 3D system,
but it is a second datum reader beside `HorizontalDatum::of_wkt1`, which a file's WKT is
read through; that is a change of what is accepted, not a fix, so a 3D frame is refused.

**What shows it.** On the rig:

- `gungnir-data` `--lib` under `crs`: 48 passed, among them
  `nad83::tests::a_source_with_no_wkt1_form_is_refused_by_name_rather_than_panicking`
  (the refusals above, and the datum still read for `EPSG:6318`, `EPSG:4269`,
  `EPSG:4978` and a `+type=crs` UTM string) and
  `proj_lock::tests::an_object_with_no_wkt1_form_is_an_error_not_a_panic` (the UTM
  operation GAP-261's test first panicked on, which PROJ reports as a
  `ConcatenatedOperation`, returns an error for WKT1, describes itself in WKT2 and still
  converts; and the same for `EPSG:4979`).
- `gungnir-app --test terrain` under `crs`: 16 passed, among them
  `a_geographic_3d_terrain_frame_is_refused_by_name_rather_than_panicking`, which
  panicked before the change.

# DEM fixtures

Generated 2026-09-06 by a script in the change that built the DEM loaders (GAP-023);
nothing here was copied from any dataset, so no licence attaches. Every byte is
described below, and the loader tests in `gungnir-data/tests/dem.rs` assert these
figures exactly (`verification-capability-table.md` §2, the `gungnir-data` row).

## The grid

Five columns by four rows, north row first, one no-data cell at row 1, column 1:

```
10    11 12 13 14
20 -9999 22 23 24
30    31 32 33 34
40    41 42 43 44
```

South-west corner at x = 500000, y = 6000000, cells 30 by 30, no-data value -9999.
Bounds are therefore [500000, 6000000] to [500150, 6000120]. Nineteen cells hold a
height; the eight 2 x 2 cell blocks that do not touch the no-data cell make sixteen
triangles.

## Files

| File | What it is | SHA-256 |
|---|---|---|
| `small.asc` | The grid as an ESRI ASCII grid: `ncols`, `nrows`, `xllcorner`, `yllcorner`, `cellsize`, `NODATA_value`, then the four rows | `649f4612161e7ffdc05f3a5927f32ace7b527e11f7d23273b7548d48ab1f1977` |
| `small.tif` | The grid as a little-endian classic TIFF, one strip of 32-bit IEEE floats, with `ModelPixelScale` (33550) = [30, 30, 0], `ModelTiepoint` (33922) = raster (0, 0) at model (500000, 6000120), a `GeoKeyDirectory` (34735) of three short keys -- `GTModelType` 1 (projected), `GTRasterType` 1 (PixelIsArea), `ProjectedCSType` 32633 -- and `GDAL_NODATA` (42113) = "-9999" | `f506703b66389040ad084f640d5b7736f976a49cb7837b91c172eb91d2fcb4e7` |
| `plain.tif` | The same raster with none of the georeferencing tags: a TIFF, not a GeoTIFF, which the loader must refuse by name | `8944fb98c2b7b8463d1053cc637325c2f55c094051f9d518e481b64a30656aa1` |

EPSG 32633 (WGS 84 / UTM zone 33N) is a plausible frame for a coastal-defence
laydown and is otherwise arbitrary; the fixture asserts that the code is carried, not
that the coordinates fall anywhere in particular.

## `small-egm2008.tif` (GAP-108, 2026-09-26)

The same raster as `small.tif`, with GDAL's own GeoTIFF keys for the compound system
EPSG:32633+3855 (WGS 84 / UTM zone 33N + EGM2008 height), so the loader's reading of a
file's own `VerticalGeoKey` is checked against keys a real tool wrote rather than ones
this workspace laid out. Made with GDAL 3.11.3 (the OSGeo image
`ghcr.io/osgeo/gdal:alpine-small-3.11.3`) from `small.tif` above, nothing else changed:

```sh
gdal_translate -q -a_srs EPSG:32633+3855 small.tif small-egm2008.tif
```

| File | What it is | SHA-256 |
|---|---|---|
| `small-egm2008.tif` | 452 bytes; the four-row, five-column raster unchanged, one strip of 32-bit floats, `GDAL_NODATA` "-9999", `ModelPixelScale` [30, 30, 1], `ModelTiepoint` raster (0, 0) at model (500000, 6000120, 0), and a `GeoKeyDirectory` of five keys -- `GTModelType` 1, `GTRasterType` 1, `GTCitation` (ASCII, "WGS 84 / UTM zone 33N + EGM2008 height"), `ProjectedCSType` 32633 and `VerticalGeoKey` 3855 | `8f9f9e16927b24e422431a1c32de7fcb93fc9dc11be9e5e8b90c3be151672bb1` |

It lies inside the committed clip of the EGM2008 grid (`../geoid/SOURCE.md`), which is
what lets `gungnir-app/tests/terrain.rs` convert it through the geoid without the 80 MB
grid.

## `usgs_1_n44w124_clip.tif` (GAP-252, D-156, D-158, 2026-10-30)

Unlike the files above, this one is real data. It is the ground the owner looks at when the
three-d scene draws terrain (`docs/design/DN-35-terrain-in-the-three-d-scene.md`
amendments 1 and 2). It is test data only: never linked into, embedded in, or shipped
with a binary.

**The source.** USGS 3D Elevation Program (3DEP), the 1 arc-second seamless DEM, tile
`n44w124`, from the National Map's own bucket. It was downloaded on 2026-10-30 with the
owner's approval. A work of the United States Government, in the public domain; no
attribution is owed.

| Field | Value |
|---|---|
| URL | `https://prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1/TIFF/current/n44w124/USGS_1_n44w124.tif` |
| `Last-Modified` when fetched | Tue, 05 Aug 2025 17:44:47 GMT |
| Bytes | 61 425 260 |
| SHA-256 | `aef61c1fb0674819a939dc134bbc14a8e534ccda3678c06ab44f21ce720b5db9` |
| Raster | 3604 x 3604, Float32, a cloud-optimised GeoTIFF (LZW, predictor 3), `AREA_OR_POINT=Area`, no-data -999999 |
| Grid | Origin (124.000556 W, 44.000556 N), cells 1/3600 degree: 43 to 44 N and 123 to 124 W with a two-cell overlap on each side |
| Horizontal | **Unqualified NAD83** (EPSG:4269), latitude and longitude in degrees |
| Vertical | **None stated in the file.** USGS publishes 3DEP heights in NAVD88 (EPSG:5703) |

Neither the realisation nor the vertical system is in the file, so a baseline that names
this DEM has to declare both: `nad83_realisation` and `vertical`. The loader refuses an
unqualified NAD83 file, or a file with no vertical system, when the baseline does not
declare them (GAP-108, GAP-200).

**How it was cut.** With GDAL 3.11.3 from the OSGeo image
`ghcr.io/osgeo/gdal:alpine-small-3.11.3`, the same tool the geoid clips used. It was run
with no network, out of the tile whose digest is above, by cell offsets so no resampling
can occur:

```sh
gdal_translate -q -srcwin 1985 671 1556 1038 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    USGS_1_n44w124.tif usgs_1_n44w124_clip.tif
```

The clip runs from (123.449167 W, 43.814167 N) at its north-west corner to
(123.016944 W, 43.525833 N) at its south-east corner. That is 1556 columns by 1038 rows,
about 35 km east to west and 32 km north to south. It is centred south-west of Eugene,
over the Coast Range foothills, rising to the south-east. Heights run from 54.3 m to
1012.6 m, mean 326 m. **No cell is no-data**, so the clip has no hole in it.

**Why there.** It lies inside the committed GEOID18 and NADCON5 clips (`../geoid/SOURCE.md`,
`../nadcon5/SOURCE.md`). Despite their file names, those clips cover 43.5 to 44.5 N and
122.5 to 123.5 W, not 43 to 45 N and 122 to 124 W. The DEM clip stays at least 2.8 km
inside them on every side, the least of it at the south edge. It holds round 1's sector (two radars of 15 km and 12 km, the
second about 6 km east of the first) around an origin at 43.67 N, 123.25 W, with about a
kilometre to spare.

| File | Bytes | SHA-256 |
|---|---|---|
| `usgs_1_n44w124_clip.tif` | 4 351 944 | `0d00c04390c5738df1a3f896322ab37d15ccefc7800b8758ffa9cafa1673ab34` |

The `tiff` crate the loader reads with (0.11.3, default features) decodes DEFLATE and the
floating-point predictor for 32-bit floats. Nothing in the workspace loads this file yet;
GAP-252's build is the first to.

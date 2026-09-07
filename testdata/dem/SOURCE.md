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

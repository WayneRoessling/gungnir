# The EGM2008 geoid clip

Test data only (GAP-108, D-121). Never linked into, embedded in, or shipped with a
binary, and **not** the grid a deployment uses: that is the full pinned file, installed
beside a deployment and never committed (`deploy/README.md`, "The EGM2008 geoid grid").

## What the full grid is

| Field | Value |
|---|---|
| File | `us_nga_egm08_25.tif` |
| What it is | NGA's EGM2008 geoid undulation on a 2.5 arc-minute grid, converted by the PROJ project to its GeoTIFF grid format ("Converted from egm08_25.gtx", per the file's own `TIFFTAG_IMAGEDESCRIPTION`) |
| Distributed by | PROJ-data, the PROJ project's own grid collection, at <https://cdn.proj.org/us_nga_egm08_25.tif> |
| Licence | Public domain: "Derived from work by NGA. Public Domain" (`TIFFTAG_COPYRIGHT` in the file) |
| Bytes | 80 585 622 |
| SHA-256 | `4191d471eefebf24091b56dbc604353cb3b8cf8cc70e448bb9ae56a272bef17a` |
| Digest's source | PROJ-data's own index, <https://cdn.proj.org/files.geojson>, which publishes this `sha256sum` for the file; the copy downloaded 2026-09-26 matched it |
| Layout | 8640 x 4321 nodes, Float32, one band `geoid_undulation` in metres, `AREA_OR_POINT=Point`, node (0, 0) at 180 W, 90 N, 1/24 degree apart; `TYPE=VERTICAL_OFFSET_GEOGRAPHIC_TO_VERTICAL`, WGS 84 (EPSG:4979) to EGM2008 height (EPSG:3855) |

The same digest is in `deploy/geoid/SHA256SUMS` and in
`gungnir_data::geoid::EGM2008_GRID_SHA256`, and a test holds those two equal.

## How the clip was cut

With GDAL 3.11.3, from the OSGeo project's own image
`ghcr.io/osgeo/gdal:alpine-small-3.11.3`, out of the pinned file above (its digest checked
first), by node offsets rather than by coordinates so no resampling can occur:

```sh
gdal_translate -srcwin 4632 816 97 73 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    us_nga_egm08_25.tif egm08_25_clip_53n56n_13e17e.tif
```

Column 4632 is 13 E ((13 + 180) x 24) and row 816 is 56 N ((90 - 56) x 24); 97 columns
and 73 rows run to 17 E and 53 N inclusive. `gdal_translate` copies every node value
bit for bit and carries the file's own metadata across, including the band description,
the unit, `AREA_OR_POINT=Point` and `TYPE`. One inherited tag is now untrue of the clip
and was left as GDAL wrote it rather than edited: `area_of_use=World`. PROJ does not read
it for `vgridshift`.

It covers the fixtures the geoid tests convert: `testdata/dem/small.tif` and
`small-egm2008.tif` (about 54.15 N, 15.00 E) and `testdata/pointcloud/five-points.las`
read as EPSG:32633 (the same place).

## Files

| File | Bytes | SHA-256 |
|---|---|---|
| `egm08_25_clip_53n56n_13e17e.tif` | 15 878 | `60a16af44ca47724fd6cbb58565104a010dd2ef8c2d5ec1c666552052fa83e10` |

## Checked against the full grid

`pyproj` 3.8.0 (PROJ 9.8.1), network off, gave the same undulation from the clip and
from the full pinned file at five points inside the clip, to 3e-14 m, and the same again
when PROJ chose its own operation for `EPSG:4326+3855` to `EPSG:4979` with only the full
file on its path. A hand bilinear interpolation of the clip's nodes as GDAL prints them
(`gdal_translate -of XYZ`) agreed to 3e-11 m. The values are transcribed into
`gungnir-data/tests/geoid.rs`, which says what each test holds them to.

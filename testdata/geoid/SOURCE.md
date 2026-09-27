# The geoid grid clips

Test data only (GAP-108 and D-121; GAP-197 and D-125). Never linked into, embedded in, or
shipped with a binary, and **not** the grids a deployment uses: those are the full pinned
files, installed beside a deployment and never committed (`deploy/README.md`, "The geoid
grids").

## What the full grids are

All three are PROJ-data's own GeoTIFF grids, from the PROJ project's grid collection at
`cdn.proj.org`, each pinned by the `sha256sum` PROJ-data's own index
(<https://cdn.proj.org/files.geojson>) publishes for it. The copy downloaded for each
change matched that digest. The same digests are in `deploy/geoid/SHA256SUMS` and in
`gungnir_data::geoid::GeoidModel`, and a test holds the two equal.

| Field | EGM2008 | EGM96 | GEOID18 (CONUS) |
|---|---|---|---|
| File | `us_nga_egm08_25.tif` | `us_nga_egm96_15.tif` | `us_noaa_g2018u0.tif` |
| What it is (the file's own `TIFFTAG_IMAGEDESCRIPTION`) | "Converted from egm08_25.gtx": NGA's EGM2008 undulation, 2.5' | "WGS 84 (EPSG:4979) to EGM96 height (EPSG:5773). Converted from egm96_15.gtx" | "NAD83(2011) (EPSG:6319) to NAVD88 height (EPSG:5703). Converted from g2018u0.gtx" |
| Licence (`TIFFTAG_COPYRIGHT`) | "Derived from work by NGA. Public Domain" | "Derived from work by NGA. Public Domain" | "Derived from work by NOAA. Public Domain" |
| Pinned for | GAP-108, 2026-09-26 | GAP-197, 2026-09-26 | GAP-197, 2026-09-26 |
| Bytes | 80 585 622 | 2 710 815 | 16 742 155 |
| SHA-256 | `4191d471eefebf24091b56dbc604353cb3b8cf8cc70e448bb9ae56a272bef17a` | `db493027562c9b004d7220fa881f5603adada4e1c5029b933fa7de4547b0e78d` | `fa9a407ac7ee3f5a3694008e4bcd09ce9cc250452f0c3b11700a4960340abce2` |
| Nodes | 8640 x 4321, 1/24 degree apart, node (0, 0) at 180 W, 90 N | 1440 x 721, 1/4 degree apart, node (0, 0) at 180 W, 90 N | 4201 x 2041, 1/60 degree apart, node (0, 0) at 130 W (stored as 230 E), 58 N |
| Source frame | WGS 84 (EPSG:4979) | WGS 84 (EPSG:4979) | NAD83(2011) (EPSG:6319) |

Every one is Float32, one band `geoid_undulation` in metres, `AREA_OR_POINT=Point`,
`TYPE=VERTICAL_OFFSET_GEOGRAPHIC_TO_VERTICAL`.

PROJ-data's index was read on 2026-09-26 for GAP-197. It holds no GEOID18 file for Alaska
or Hawaii (their NAVD88 grids there are GEOID12B's, `us_noaa_g2012ba0.tif` and
`us_noaa_g2012bh0.tif`), and its `us_noaa_g2018p0.tif` relates Puerto Rico and the Virgin
Islands to PRVD02 (EPSG:6641), not to NAVD88. So GEOID18 is pinned for the conterminous
United States only, which is what the one CONUS file covers.

## How the clips were cut

With GDAL 3.11.3, from the OSGeo project's own image
`ghcr.io/osgeo/gdal:alpine-small-3.11.3`, out of each pinned file (its digest checked
first), by node offsets rather than by coordinates so no resampling can occur:

```sh
gdal_translate -srcwin 4632 816 97 73 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    us_nga_egm08_25.tif egm08_25_clip_53n56n_13e17e.tif
gdal_translate -srcwin 772 136 17 13 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    us_nga_egm96_15.tif egm96_15_clip_53n56n_13e17e.tif
gdal_translate -srcwin 390 810 61 61 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    us_noaa_g2018u0.tif g2018u0_clip_43n45n_124w122w.tif
```

- **EGM2008**: column 4632 is 13 E ((13 + 180) x 24) and row 816 is 56 N ((90 - 56) x
  24); 97 columns and 73 rows run to 17 E and 53 N inclusive.
- **EGM96**: column 772 is 13 E ((13 + 180) x 4) and row 136 is 56 N ((90 - 56) x 4);
  17 columns and 13 rows run to 17 E and 53 N inclusive. The same area as the EGM2008
  clip, so the same fixtures convert through either.
- **GEOID18**: column 390 is 123.5 W ((236.5 - 230) x 60, the file storing longitude
  east of Greenwich from 0 to 360) and row 810 is 44.5 N ((58 - 44.5) x 60); 61 columns
  and 61 rows run to 122.5 W and 43.5 N inclusive. It covers the Autzen capture
  (`testdata/pointcloud/autzen-classified.copc.laz` and `autzen-geokeys.las`, about
  44.05 N, 123.07 W).

`gdal_translate` copies every node value bit for bit and carries each file's own metadata
across, including the band description, the unit, `AREA_OR_POINT=Point` and `TYPE`.
Inherited tags now untrue of a clip -- `area_of_use` -- were left as GDAL wrote them
rather than edited. PROJ does not read them for `vgridshift`.

The EGM2008 and EGM96 clips cover the fixtures the geoid tests convert in the Baltic:
`testdata/dem/small.tif` and `small-egm2008.tif` (about 54.15 N, 15.00 E) and
`testdata/pointcloud/five-points.las` read as EPSG:32633 (the same place).

## Files

| File | Bytes | SHA-256 |
|---|---|---|
| `egm08_25_clip_53n56n_13e17e.tif` | 15 878 | `60a16af44ca47724fd6cbb58565104a010dd2ef8c2d5ec1c666552052fa83e10` |
| `egm96_15_clip_53n56n_13e17e.tif` | 1 425 | `9b8e9b6c7811e88b72af9191e118e9c43ed03630b84ea2b3789647e925300b19` |
| `g2018u0_clip_43n45n_124w122w.tif` | 9 165 | `a93c8ca6a47cc3d5a58ffeeac469aa9c8c9ca2400a0113a1976eb0a4bc9b16eb` |

## Checked against the full grids

`pyproj` 3.8.0 (PROJ 9.8.1, a different PROJ from the 9.6.2 `proj-sys` builds), network
off, each full pinned file on its data path:

- **EGM2008** (GAP-108): the same undulation from the clip and from the full file at five
  points inside the clip, to 3e-14 m, and the same again when PROJ chose its own
  operation for `EPSG:4326+3855` to `EPSG:4979`. A hand bilinear interpolation of the
  clip's nodes as GDAL prints them (`gdal_translate -of XYZ`) agreed to 3e-11 m.
- **EGM96** (GAP-197): the same undulation three ways at five points -- PROJ's own choice
  for `EPSG:4326+5773` to `EPSG:4979` ("Inverse of WGS 84 to EGM96 height (1)"), the
  written-out `vgridshift` pipeline on the full file, and on the clip -- to 7e-14 m.
- **GEOID18** (GAP-197): the same three ways at five points around Autzen -- PROJ's own
  choice for `EPSG:6318+5703` to `EPSG:6319` ("Inverse of NAD83(2011) to NAVD88 height
  (3)"), the pipeline on the full file, and on the clip -- to 3e-14 m. PROJ's own choice
  for the Autzen capture's whole compound WKT to `EPSG:4979` is the same `vgridshift` on
  this file, after the Lambert inverse and the US survey foot, with null steps between
  NAD83, NAD83(2011) and WGS 84.

The values are transcribed into `gungnir-data/tests/geoid.rs` and
`gungnir-data/tests/pointcloud_crs.rs`, which say what each test holds them to.

# The NADCON5 grid clips

Clips of the four NADCON5 grids GAP-200 pins (D-128), so the NAD83 realisation chain is
tested in every build with the `crs` feature without the 95 MB of full grids. The full
grids are deployment artifacts, never committed, pinned by digest in
`deploy/geoid/SHA256SUMS` and in `gungnir_data::nad83` alike (`deploy/README.md`, "NAD83
positions and the frame epoch"); `ci.yml`'s `proj-crs` job fetches and checks them.

| Clip | Cut from | Clip SHA-256 |
|---|---|---|
| `nadcon5_1986_harn_clip_43n45n_124w122w.tif` | `us_noaa_nadcon5_nad83_1986_nad83_harn_conus.tif` | `2c1de773467589732394ca55fba70e194969b23cbcfb78c9b0655125804c1828` |
| `nadcon5_harn_fbn_clip_43n45n_124w122w.tif` | `us_noaa_nadcon5_nad83_harn_nad83_fbn_conus.tif` | `efd6c9de5569ab7e6451be9a5deda1cf079c75ffee8a74ac10c5b80ab29bb639` |
| `nadcon5_fbn_2007_clip_43n45n_124w122w.tif` | `us_noaa_nadcon5_nad83_fbn_nad83_2007_conus.tif` | `2cf42b29211aaf3a6b0ac20ccbfcc6bb623c1badb580770a1c263edae20f8a80` |
| `nadcon5_2007_2011_clip_43n45n_124w122w.tif` | `us_noaa_nadcon5_nad83_2007_nad83_2011_conus.tif` | `a460457996126ebef3a71232aa7f25e50bef8207516ab0e81648d7189e1eebe5` |

## Source and licence

NOAA's National Geodetic Survey NADCON5 (2016-09-01 release), as PROJ-data redistributes
it: `https://cdn.proj.org/<file>`, each file's SHA-256 matching the one PROJ-data's index
(`files.geojson`) publishes for it when read on 2026-10-23. NADCON5 is a work of the United
States Government and in the public domain; PROJ-data marks the files open-licence.

## How the clips were cut

With GDAL 3.11.3, from the OSGeo project's own image
`ghcr.io/osgeo/gdal:alpine-small-3.11.3`, out of each pinned file (its digest checked
first), by node offsets rather than by coordinates so no resampling can occur -- the same
procedure as `testdata/geoid/SOURCE.md`:

```sh
gdal_translate -srcwin 30 110 21 21 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    us_noaa_nadcon5_nad83_1986_nad83_harn_conus.tif nadcon5_1986_harn_clip_43n45n_124w122w.tif
gdal_translate -srcwin 90 330 61 61 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    us_noaa_nadcon5_nad83_harn_nad83_fbn_conus.tif nadcon5_harn_fbn_clip_43n45n_124w122w.tif
gdal_translate -srcwin 90 330 61 61 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    us_noaa_nadcon5_nad83_fbn_nad83_2007_conus.tif nadcon5_fbn_2007_clip_43n45n_124w122w.tif
gdal_translate -srcwin 90 330 61 61 -co COMPRESS=DEFLATE -co PREDICTOR=3 \
    us_noaa_nadcon5_nad83_2007_nad83_2011_conus.tif nadcon5_2007_2011_clip_43n45n_124w122w.tif
```

- **1986 to HARN** is a 0.05-degree grid whose first node is 125 W, 50 N: column 30 is
  123.5 W ((125 - 123.5) / 0.05) and row 110 is 44.5 N ((50 - 44.5) / 0.05); 21 columns
  and 21 rows run to 122.5 W and 43.5 N inclusive.
- **The other three** are one-arc-minute grids with the same first node: column 90 is
  123.5 W and row 330 is 44.5 N; 61 columns and rows run to 122.5 W and 43.5 N inclusive.

Every clip covers the same area as the GEOID18 clip, and so the Autzen capture
(`testdata/pointcloud/autzen-classified.copc.laz` and `autzen-geokeys.las`, about 44.05 N,
123.07 W). `gdal_translate` copies every node value bit for bit and carries each file's
own metadata across, including the band descriptions and `TYPE`.

## How they were checked

With `pyproj` 3.8.0 (PROJ 9.8.1), network off, the full grids on its data path: the
pipeline `gungnir_data::nad83` writes for each realisation, over the full grids, agreed
with PROJ's own choice of operations from each realisation's geographic system to
NAD83(2011) and on to ITRF2014 at epoch 2020.0 to within 1e-9 m at four points around
Autzen. The same pipeline over these clips agreed with the full grids to within 0.6 mm
(the 1986 to HARN step, whose interpolation near a clip's edge reaches nodes the clip does
not hold) and to within 0.01 mm for the other three.

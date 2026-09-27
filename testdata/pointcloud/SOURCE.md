# Point-cloud fixtures

Two kinds of fixture in this directory, on two different footings.

## `five-points.las`

Generated 2026-09-06 by a script in the change that built the LAS loader (GAP-023);
nothing here was copied from any dataset, so no licence attaches. The loader test in
`gungnir-data/tests/pointcloud.rs` asserts these figures exactly
(`verification-capability-table.md` §2, the `gungnir-data` row).

LAS 1.2, point data format 0, one 227-byte header and five 20-byte records, little
endian. Scale 0.01 m on every axis, offset (500000, 6000000, 0), so the integer
coordinates are centimetres from that offset. The header's bounds are the true min and
max of the five points.

| # | x | y | z | intensity | classification |
|---|---|---|---|---|---|
| 1 | 500010.00 | 6000020.00 | 12.50 | 100 | 2 (ground) |
| 2 | 500011.25 | 6000020.00 | 12.75 | 120 | 2 |
| 3 | 500010.00 | 6000021.50 | 13.00 | 140 | 5 (high vegetation) |
| 4 | 500012.00 | 6000022.00 | 18.25 | 160 | 6 (building) |
| 5 | 500013.50 | 6000019.00 | 12.00 | 180 | 2 |

Bounds: x 500010.00 to 500013.50, y 6000019.00 to 6000022.00, z 12.00 to 18.25. Every
record is return 1 of 1, scan angle 0, point source id 1.

SHA-256: `324915ee19655acaedd9f8dca90708cdb8e391b6218ae00542b52b9f1ad70425`

## `autzen-classified.copc.laz`

Copied 2026-09-07 (GAP-023's closing action): a real capture with a real,
externally-built COPC octree hierarchy, so `load_copc_bounded`'s happy path is checked
against a hierarchy this workspace did not shape to match its own assumptions --
`five-points.las` above and every other fixture this crate generates could not stand in
for that, whatever LAS content they held. Test data only; never linked into, embedded
in, or shipped with a binary.

**Origin.** The Autzen Stadium LiDAR data was captured by Aaron Reyna of Watershed
Sciences, Inc. in 2010 for libLAS data testing; in 2021 Max Sampson of Hobu, Inc.
manually classified 21 categories of objects on it, producing `autzen-classified.laz`.
The file vendored here is that same classified point set, re-encoded into the COPC
container (an octree-organised LAZ) for use as a COPC example -- same points, same
classifications, a different on-disk hierarchy over them. Neither of this crate's own
fixtures nor `las::copc::CopcReader`'s doctest fixture has a comparable third-party
history, which is the reason for reaching outside this workspace at all.

**Licence.** Creative Commons Attribution 4.0 International (CC-BY-4.0), stated in
`LICENSE` at the repository root; copied here verbatim as `PDAL-DATA-LICENSE.txt`.
Attribution: Autzen Stadium LiDAR, Watershed Sciences, Inc. (2010) and Hobu, Inc.
(2021 classification), via the PDAL project's public test-data repository.

**Provenance.**

| Field | Value |
|---|---|
| Repository | <https://github.com/PDAL/data> |
| Path in that repository | `autzen/autzen-classified.copc.laz` |
| Repository `main` when copied | `ce0024257c573526389c4db9ab26e82739b8aaa9` (2024-12-31) |
| Last commit to touch the file | `360327d2ae791b9d52c57b610a5a6b5c1b08c878` (2024-12-31, "put back old autzen stuff at same location") |
| Copied | 2026-09-07, by direct download over the repository's Git LFS media endpoint (`raw.githubusercontent.com` serves only the LFS pointer text for this file, not its content) |

The upstream repository stores this file under Git LFS; GitHub's own git-blob id for the
path (`5f07c0d69555ca048fd2f2640c850a46229cb5ae`) names the 133-byte **pointer** object,
not the content. The pointer's own stated SHA-256 and byte count are what the table
below repeats, so the identity that matters -- the actual bytes read here -- is the one
recorded, not the wrapper git happens to store it as.

## Files

| File | Bytes | SHA-256 |
|---|---|---|
| `autzen-classified.copc.laz` | 81 123 042 | `db2d56cdfa058bffccdc5d6019dae2fc9c6a551df10a5523c06c76a3e25a27fa` |
| `PDAL-DATA-LICENSE.txt` | 18 658 | `f5b745ef98087f531e719ee8ca6a96809444573ecc7173c6fa68eaad39b3cc3f` |

Both are byte-for-byte copies; nothing was truncated or otherwise edited.

## What the fixture holds, for the test that reads it

LAS 1.4, extended point format (GPS time, colour, no waveform, no NIR), LAZ-compressed,
10,653,336 points. File bounds (in its own coordinate system, below): x [635577.79,
639003.73], y [848882.15, 853537.66], z [406.14, 615.26]. `gungnir-data/tests/pointcloud.rs`'s happy-path test queries the round-number
box x [637200, 637300], y [851100, 851200], z [400, 620] -- chosen from these bounds,
not from what the query happens to return -- and asserts the exact point count (4767)
and classification breakdown (2 unclassified, 4499 ground, 114 high vegetation, 8
overhead structure, 144 car) that box returns.

### The coordinate reference system it declares (GAP-102)

**This is the real-world CRS GAP-102 needed, and the reason no new fixture was
authored for it.** The file carries a WKT VLR of its own -- `LASF_Projection`, record
2112, 993 bytes, with bit 4 of the header's global encoding set as LAS 1.4 requires --
and it declares a **compound** system:

| Part | System | EPSG | Unit |
|---|---|---|---|
| Horizontal | NAD83 / Oregon GIC Lambert (ft) | 2992 | international foot, 0.3048 m (EPSG 9002) |
| Vertical | NAVD88 height (ftUS) | 6360 | **US survey foot**, 1200/3937 m (EPSG 9003) |

`gungnir-data/tests/pointcloud_crs.rs` reads all of this out of the file rather than
trusting this table.

**Two things about it are worth stating, because both are easy to get wrong.**

First, **the two axes are not in the same foot.** The horizontal axes are international
feet and the heights are US survey feet; the two differ by about two parts per million.
An earlier version of this file said "US survey feet per the original capture" of the
whole thing, which is right about the heights and wrong about the eastings and
northings. The loader reads each from the node that declares it (`VERT_CS`'s own `UNIT`
for the vertical), so a reader who takes the horizontal unit for both is the mistake the
code is written to avoid.

Second, **converting this file's heights does not give heights above the WGS-84
ellipsoid.** NAVD88 is a gravity-related datum, and the separation between it and the
ellipsoid in this part of Oregon is of the order of -22 m. `libproj` applies that
separation only when it has the relevant vertical-datum grid, which a deployment that
never fetches grids over the network does not; without it PROJ converts the unit and
stops. Since GAP-108 (D-121) this workspace does not stop there. Between GAP-108 and
GAP-197 this file's NAVD88 heights were **refused by name**, no NAVD88 grid being pinned;
since GAP-197 (D-125) they convert through NOAA's GEOID18 grid, pinned for the
conterminous United States, and are refused by name, naming that grid, only where a
deployment has not installed it (`gungnir-data/src/geoid.rs`;
`gungnir-app/tests/pointcloud_crs.rs` checks both through a real tick). GEOID18 gives a
NAD83(2011) ellipsoidal height, which the desktop reads as WGS 84's to the metre level
(`gungnir-data/src/geoid.rs` states the figures). The horizontal-only test still holds
the datum still on purpose, so the Lambert inverse and the vertical unit are checked on
their own too.

The bounds above, put through the horizontal-only conversion, are longitude
[-123.07498674, -123.06251260], latitude [44.04971882, 44.06278031], height
[123.79171958, 187.53162306] -- an independent `pyproj` computation, recorded in
`gungnir-data/tests/pointcloud_crs.rs` with what it checked and to what tolerance.
Through GEOID18 the box's centre (X 637250, Y 851150, Z 500 ftUS) is 129.06263075346988 m
above the ellipsoid, GEOID18's undulation there being -23.33767404713973 m.

## `autzen-geokeys.las`

Generated 2026-09-26 for GAP-197 by a short script in that change (not committed, the
five-point fixture's precedent); nothing in it was copied from any dataset, so no licence
attaches. It exists to test what no committed fixture could: a LAS 1.0-1.3 file whose
`GeoTIFF` key directory states a vertical system (GAP-102 item (2)).

LAS 1.2, point data format 0, one 227-byte header, two VLRs and five 20-byte records,
little endian. Scale 0.01 on every axis, offset (637000, 851000, 0). The coordinates are
the Autzen capture's own numbers, in its own system -- NAD83 / Oregon GIC Lambert in
international feet, NAVD88 heights in US survey feet -- so they convert through the same
GEOID18 clip as the capture:

| # | x (ft) | y (ft) | z (ftUS) | intensity | classification | what it is |
|---|---|---|---|---|---|---|
| 1 | 637250.00 | 851150.00 | 500.00 | 100 | 2 | the centre of the capture's test query box |
| 2 | 635577.79 | 848882.15 | 406.14 | 120 | 2 | the capture's minimum corner |
| 3 | 639003.73 | 853537.66 | 615.26 | 140 | 5 | the capture's maximum corner |
| 4 | 637200.00 | 851100.00 | 450.50 | 160 | 6 | the query box's corner |
| 5 | 638000.00 | 852000.00 | 420.25 | 180 | 2 | between |

**The key directory is GDAL's, not this workspace's.** GDAL 3.11.3
(`ghcr.io/osgeo/gdal:alpine-small-3.11.3`) wrote a two-by-two GeoTIFF with
`gdal_create -a_srs EPSG:2992+6360`, and its `GeoKeyDirectoryTag` and
`GeoAsciiParamsTag` were copied byte for byte into the two `LASF_Projection` VLRs
(records 34735 and 34737), which is where the LAS 1.2 specification puts them:

| Key | Value |
|---|---|
| directory header | version 1, revision 1.1, five keys (GeoTIFF 1.1) |
| `GTModelTypeGeoKey` (1024) | 1, projected |
| `GTRasterTypeGeoKey` (1025) | 1 |
| `GTCitationGeoKey` (1026) | "NAD83 / Oregon GIC Lambert (ft) + NAVD88 height (ftUS)\|" |
| `ProjectedCRSGeoKey` (3072) | 2992 |
| `VerticalGeoKey` (4096) | 6360 |

GDAL writes **no** `VerticalUnitsGeoKey` and no `ProjLinearUnitsGeoKey` for this compound
system: the codes fix the units. So the loader reads the US survey foot from the code
6360 itself. For comparison, GDAL writes a GeoTIFF 1.0 directory with
`ProjLinearUnitsGeoKey` (3076 = 9002 for 2992, 9001 for a UTM zone) for a horizontal
system alone, and LAStools writes `VerticalUnitsGeoKey` (4099) beside `VerticalGeoKey`;
`gungnir-data/src/pointcloud/crs.rs`'s unit tests carry those two directories too.

`pyproj` 3.8.0, with the full pinned GEOID18 grid, took `EPSG:2992+6360` to `EPSG:4979`
for the five points; `gungnir-data/tests/pointcloud_crs.rs` and
`gungnir-app/tests/pointcloud_crs.rs` hold the conversion to those values.

SHA-256: `3c754bf7d97c98bcdc3d8e9effa773a3994202114db1995f6439f8237ff6508c` (539 bytes).

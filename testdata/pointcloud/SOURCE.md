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
stops, and so does this workspace. `gungnir-data/src/pointcloud/crs.rs::to_local_enu`
says the same thing at the point where it matters.

The bounds above, put through that conversion, are longitude [-123.07498674,
-123.06251260], latitude [44.04971882, 44.06278031], height [123.79171958,
187.53162306] -- an independent `pyproj` computation, recorded in
`gungnir-data/tests/pointcloud_crs.rs` with what it checked and to what tolerance.

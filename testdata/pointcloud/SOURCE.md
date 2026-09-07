# Point-cloud fixtures

Generated 2026-09-06 by a script in the change that built the LAS loader (GAP-023);
nothing here was copied from any dataset, so no licence attaches. The loader test in
`gungnir-data/tests/pointcloud.rs` asserts these figures exactly
(`verification-capability-table.md` §2, the `gungnir-data` row).

## `five-points.las`

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

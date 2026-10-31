# GAP-227: MGRS, UTM and UPS in gungnir-coord (DN-37 increment 1)

The first increment of `docs/design/DN-37-symbology-cursor-readout-and-declutter.md` to be
built is the one the cursor readout stands on, not the symbols: D-170's MGRS, held to
`GeographicLib`'s `GeoConvert`. It is built first because it needs nothing the standard for
the symbols (MIL-STD-2525D) has to supply, and the readout (the next increment) needs it. The
symbols wait on the standard's own tables, which are not in the repository.

**What is built.** `gungnir_coord::grid`, one way, geodetic to grid:

- `utm_ups(lat_deg, lon_deg)` gives the zone, hemisphere, easting and northing. UTM by the
  Krüger series to sixth order; UPS by the conformal-latitude form; the Norway and Svalbard
  zones; UTM from -80 inclusive to 84 exclusive.
- `mgrs(lat_deg, lon_deg, digits)` and `mgrs_of(Geodetic, digits)` give the reference,
  truncated (never rounded) to 0 to 5 digits for each axis. Five is one metre.
- `LocalFrame::mgrs(enu, digits)` in `gungnir-model` is the surface the viewport will call, so
  the viewport adds no dependency edge. `GridError` is re-exported beside `Geodetic`.

There is no inverse and no MGRS parser. The readout only writes a reference. A parser
would be a second thing to hold to an oracle for nothing the picture uses.

**The oracle.** `testdata/oracles/tools/gen_geoconvert_fixtures.py` runs `GeoConvert` 2.5.2
(conda-forge `geographiclib-cpp`) over 2 953 cases and writes `testdata/oracles/coord/geoconvert.json`:
every UTM zone's boundary at three latitudes with a hair either side; every latitude band's
edge; the equator, including a negative zero, which `GeoConvert` reads as south; Norway's
and Svalbard's edges; both polar caps at the four meridians that cut them and at both
poles; nine places; and seeded sweeps of 1 500 UTM points and 300 for each cap. Each case
records the zone, hemisphere, easting and northing (to 10 micrometres) and the MGRS at 5, 3 and
0 digits. `gungnir-coord/tests/geoconvert_diff.rs` holds the crate to **the same string and a
millimetre**. It passes on all of them; the worst UTM or UPS position error is 5e-6 m, which is the fixture's
own printed precision. Changing one of the Krüger series' third-order coefficients makes it
fail, so the test sees the series and not only the lettering.

**What the oracle corrected.** Three things written from memory were wrong or missing, and
the fixture caught each before the test could pass:

1. UPS's south-west column letters are twelve (`JKLPQRSTUXYZ`), not ten. The position the
   first run refused was a southern point at -80.000001, -179.999999.
2. A southern position exactly on the equator has the false northing exactly, and MGRS
   keeps it in the southern grid, in the last square before the northern one (`32MPE...99999`,
   not `32MPF...00000`).
3. A negative-zero latitude takes the southern band, M, not the northern band, N.

**Flagged for the owner.** This is numerics in `gungnir-coord`, and `docs/agentic-workflow.md`
lists numerical stability as human-owned, so the series coefficients, the truncation and
the three rules above want a human reading. The row is **not** entered in
`docs/verification-capability-table.md`: a pass criterion there is human-owned, and DN-37 §6.1
row d is still the draft the note calls it. The test is what the row would run. Nothing about
signatures is stated here; `docs/signatures.md` is the ledger.

**What the owner looks at on a display.** Nothing. This increment draws nothing and changes
no renderer. The readout that shows it is the next increment, and its checklist is in that
record.

**Not done.** UTM south of -80 and north of 84 is UPS, as the standard has it; a position
beyond about ninety degrees of its zone's central meridian is refused with `OutsideZone`
(the Norway and Svalbard widenings reach nine degrees, well inside what the series holds).
`LocalFrame::mgrs` takes the geodetic position through `to_geodetic`, so a position far from
the local origin is as good as `gungnir-coord`'s conversion.

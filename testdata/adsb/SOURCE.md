# Origin of the ADS-B captures in this directory

Test fixtures only. These files are never linked into, embedded in, or shipped with a
binary. Copied 2026-09-06 under GAP-010, by the open-source-consensus route the owner
approved, on the terms `docs/design/external-standards.md` §1.5 set for the ASTERIX
capture and §3.3 applied to the AIS logs:

- Test data only, and **permissively licensed** — unlike the ASTERIX captures, neither of
  these is copyleft, so the §1.5 conditions are met with room to spare.
- Provenance recorded here: the repository, the commit, the path in that repository, the
  upstream blob id, the SHA-256 of the copy, and what was changed in copying.
- Decoding them is a check against real transmitters, **not** a substitute for a
  specification — and here there is no specification to substitute for. No ADS-B document
  is both free to obtain and permissively licensed, so `gungnir_interop::adsb` pins none
  and is gated against two open-source decoders instead. That is agreement with the
  **open-source consensus, not conformance**, and the verification-capability-table row
  says so in its oracle column.

Two captures rather than one, and from two layers, because GAP-010's closing action asks
for two independent recordings: a shared quirk of one receiver cannot then carry the gate.

## Sources

| Field | `lax-messages-first40000.txt` | `modes1.bin` |
|---|---|---|
| Repository | <https://github.com/rsadsb/adsb_deku> | <https://github.com/antirez/dump1090> |
| Path in that repository | `libadsb_deku/tests/lax-messages.txt` | `testfiles/modes1.bin` |
| Repository state when copied | master `e2bdf1e08d1ef1a8dbdea25f415cbd50170de60e` (2025-12-29) | master `efe64db3c6ba1520291331628a33c1e208e851a6` (2026-02-15) |
| Last commit to touch the file | `a8e4bb2cef013aac4de33a7d35bba19b7edfd544` (2022-01-12) | `7ca5a4b3a40293343be92f4b9840259935340597` (2013-01-05) |
| Licence | **MIT** (`LICENSE` at the repository root; copied here as `ADSB-DEKU-LICENSE.txt`) | **BSD 3-Clause** (`LICENSE` at the repository root; copied here as `DUMP1090-LICENSE.txt`) |
| Copyright line in that licence | `Copyright (c) 2022 Wayne Campbell` | `Copyright (c) 2012-present, Salvatore Sanfilippo` |
| Copied | 2026-09-06, by direct download at the commit above | 2026-09-06, by direct download at the commit above |

Neither repository records where or when the recording was made beyond what its own file
name says, so nothing more is claimed here: the AVR corpus is named for Los Angeles by
its upstream file name, and the IQ capture carries no place at all.

## Files

| File | Bytes | SHA-256 | Upstream git blob | What it holds |
|---|---|---|---|---|
| `lax-messages-first40000.txt` | 881 726 | `61a81b462cc99c71257cd52194f9fd29233b1ca7d6132693bb1c0550f4def13c` | (a prefix; see below) | 40 000 AVR frames, `*<hex>;` one per line, LF endings |
| `modes1.bin` | 713 736 | `3a33e16025da8669149c780075950b4e908ca036ea21f9583c113f60d5fb3094` | `62f7f97e41c0ad19d03f7bfd16da920c9b6b096b` | 356 868 IQ sample pairs, 8-bit unsigned, 2 MS/s: 0.178 s of 1090 MHz |
| `ADSB-DEKU-LICENSE.txt` | 1 071 | `f7da773cec0de4be0124b7d81ca74856abb299c85f4fafc1315fba0be787d703` | `1c094a5577ce37ceb547c323a44ae3f4ebb461b8` | The MIT text, verbatim |
| `DUMP1090-LICENSE.txt` | 1 534 | `77daa88fed5140565bfbf8bfc7f7dd36160651fd1de04f4d31dd29fc3210afce` | `f9173f8a0820621c5f9406046541c4d46619c5d0` | The BSD 3-Clause text, verbatim |

`modes1.bin` and both licence texts are byte-for-byte copies. The AVR corpus is **not**;
the next section says exactly how it differs.

## The AVR corpus is truncated, and here is how to regenerate it

Upstream `lax-messages.txt` is **4 710 276 bytes and 215 606 frames**
(SHA-256 `4272252e9b2a9c19674cf0886729eb35e5ba6175b6bacf669b27483afa15b0fc`, git blob
`a4b05f23c85e6a462b82551b9f2e88d3817a352a`). That is five times the size of everything
else under `testdata/` put together, for a corpus whose message mix repeats every few
thousand frames. **The copy here is the first 40 000 lines and nothing else has been
changed** — no reordering, no filtering, no reformatting:

```bash
curl -O https://raw.githubusercontent.com/rsadsb/adsb_deku/a8e4bb2cef013aac4de33a7d35bba19b7edfd544/libadsb_deku/tests/lax-messages.txt
head -n 40000 lax-messages.txt > lax-messages-first40000.txt
```

**What the truncation costs, stated rather than glossed.** The whole file holds 74 641
extended squitters across fourteen type codes; the prefix holds 13 324 across ten. The
four it loses are all rare, and one of them matters:

| Type code | In the whole file | In the prefix | What it is |
|---|---|---|---|
| 7 | 1 | 0 | **Surface position.** The only surface frame in 215 606, at line 64 305 |
| 10 | 5 | 0 | Airborne position, barometric — same layout as type codes 11 and 12, which the prefix has 4 914 of |
| 13 | 10 | 0 | Airborne position, barometric — as above |
| 22 | 5 | 0 | **Airborne position, GNSS height.** The only frames in the file that exercise that branch |

So two decode paths — surface position, and the GNSS-height variant of airborne position —
are **not** exercised by the vendored corpus and would not have been meaningfully
exercised by the whole file either, at one and five frames. Both are gated instead by the
published CPR vectors in `gungnir-interop/tests/adsb_cpr.rs` and the unit tests in
`gungnir_interop::adsb::messages`, and `tests/adsb_fixtures.rs` asserts that the surface
count is zero so the absence is on the record rather than left to be noticed.

## What is in the copies

`lax-messages-first40000.txt` is **unfiltered receiver output**: two thirds of it is not
extended squitters at all, and much of that is noise a preamble detector let through. That
is what a real 1090 MHz feed looks like, and it is why the decoder counts every frame it
does not read rather than filtering silently.

| Downlink format | Frames | What it is |
|---|---|---|
| 0 | 12 938 | Short air-air surveillance |
| 4 | 4 265 | Surveillance altitude reply |
| 5 | 81 | Surveillance identity reply |
| 11 | 8 307 | All-call reply (4 765 of them clear their parity, so carry interrogator identifier zero) |
| 16 | 838 | Long air-air surveillance |
| **17** | **13 187** | **Extended squitter** |
| **18** | **137** | **Extended squitter, non-transponder** (control fields 1, 5 and 6) |
| 20 | 185 | Comm-B altitude reply |
| 21 | 62 | Comm-B identity reply |

All 13 324 extended squitters clear their parity. They carry 82 distinct ICAO addresses
and these type codes:

| Type code | Frames | Decoded by this build |
|---|---|---|
| 3, 4 | 2, 486 | yes — identification |
| 11, 12 | 4 872, 42 | yes — airborne position, barometric |
| 18 | 7 | yes — airborne position, barometric |
| 19 | 4 901 | yes — airborne velocity |
| 24 | 122 | no — surface system status, carried and counted |
| 28 | 500 | no — aircraft status, carried and counted |
| 29 | 1 405 | no — target state and status, carried and counted |
| 31 | 987 | no — aircraft operation status, carried and counted |

`modes1.bin` is a 0.178 s recording of one aircraft. `gungnir_interop::adsb::demod`
recovers **198 frames whose parity clears without an address**: 141 extended squitters
(type code 4 nine times, 11 sixty-eight times, 19 sixty-four times) and 57 all-call
replies, all from ICAO **4D2023**. The surveillance replies in the capture are counted and
not returned, because their parity is mixed with the aircraft address and cannot be
checked without a list of addresses already known to be in the air.

## What decoding them established (2026-09-06)

`gungnir_interop::adsb` reads all 13 324 extended squitters of the AVR corpus and all 141
of the demodulated IQ capture, and **agrees field for field with both `rs1090` 0.6.0 and
`adsb_deku` 0.7.1** (`gungnir-interop/tests/adsb_fixtures.rs`). Three differences were
found and are recorded in that test rather than smoothed over, because two of them are
the oracles' bugs and finding them is the argument for gating against two decoders rather
than one:

- **`adsb_deku` reads seven callsign characters where the field holds eight**
  (`aircraft_identification_read`, `for _ in 0..=6`). Its callsign is a prefix of the
  real one; the test compares it as a prefix.
- **`adsb_deku` carries the altitude as `u16`** and so cannot represent the negative
  altitudes the Q-bit encoding allows for airports below sea level. No frame in either
  capture is below sea level, so the comparison was never skipped in practice.
- **`adsb_deku::AirborneVelocity::calculate` returns nothing at all when only the vertical
  rate is unavailable**, so one frame of the corpus yields no speed comparison against it.
  Counted; the same frame is compared against `rs1090`.

None of the three is a disagreement about a value. Every field both oracles do produce
matches.

## How to use them

- A decoder that reads these files must read **every** extended squitter in them without
  error. That is the "does it read a real transmitter" case a synthetic corpus cannot give.
- **A green differential run is not conformance.** The two oracles are only partly
  independent (`external-standards.md` §4), so a misreading shared by both would pass. The
  parity and the position decoding are gated by arithmetic instead
  (`tests/adsb_crc.rs`, `tests/adsb_cpr.rs`), because those two are checkable without
  asking a decoder anything.
- The AVR corpus is text and can be read line by line; `modes1.bin` needs a demodulator,
  and `gungnir_interop::adsb::demod` is one.
- Re-verify the hashes above before trusting a copy that has passed through anything other
  than version control.

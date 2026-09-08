# Origin of the ASTERIX captures in this directory

Test fixtures only. These files are never linked into, embedded in, or shipped with a
binary. Their use as fixtures was decided by the owner on 2026-09-06; the decision and its
conditions are in `docs/design/external-standards.md` §1.5.

## Source

| Field | Value |
|---|---|
| Repository | <https://github.com/CroatiaControlLtd/asterix> |
| Commit | `790bca7ed96be11bb001c3c395092f61851d9af7` (master, committed 2025-12-03) |
| Path in that repository | `asterix/sample_data/` |
| Licence of that repository | GPL-2.0 (`LICENSE` at the repository root) |
| Copied | 2026-09-06, by direct download of each file at the commit above |

The repository is Croatia Control's ASTERIX decoder. The sample files are its own test
data; no statement of the radar or date they were recorded from accompanies them, so none
is claimed here.

## Files

| File | Bytes | SHA-256 | What it holds |
|---|---|---|---|
| `cat048.raw` | 48 | `17481bd2955d046efc5aac1022b4f5879ab02456060c419503aded54bf2e5efd` | One Category 048 data block: category byte 48, declared length 48 |
| `cat034.raw` | 16 | `a5d24d471cc0fac2554924cbd6aacbf532be96204b4fb335f6275b9a1a0e5766` | One Category 034 data block: category byte 34, declared length 16 |
| `cat_034_048.pcap` | 12770 | `7f4e9a37641bfa27022ee95ba52f178e68260c1b7e83d6a0e4720a96b3a2cc3d` | Libpcap capture, little-endian, microsecond timestamps; 100 UDP datagrams over Ethernet and IPv4 holding 120 ASTERIX data blocks: 86 of Category 048 and 34 of Category 034 |
| `COPYING` | 17984 | `edaef632cbb643e4e7a221717a6c441a4c1a7c918e6e4d56debc3d8739b233f6` | The GPL-2.0 text, verbatim, fetched 2026-09-07 from <https://www.gnu.org/licenses/old-licenses/gpl-2.0.txt> |

The structural checks in the last column were made on copy (category byte, declared block
length against file length, pcap magic, block count per category). The edition the
source decoder was written against is not recorded in its repository.

## Why `COPYING` is here (added 2026-09-07)

GPL-2.0 section 1 conditions redistribution of these files on the license text
travelling with them, and it was missing — unlike `../adsb/` and `../ais/`, which each
carry their upstream text. That was invisible while this repository was private and
becomes a defect on publication, so the text was added when the workspace license was
set.

The workspace is `AGPL-3.0-or-later` (`../../LICENSE`); these four files are not, and
`../../NOTICE` names them as an exception. The two coexist as an aggregate: separate
works distributed together, not combined into one program. Nothing in
`gungnir-interop` derives from the Croatia Control decoder's source — the Category 048
decoder was built to edition 1.32 independently, and these files are only ever read as
fixtures.

## What decoding it established (2026-09-06)

The Category 048 decoder built to edition 1.32 (`gungnir-interop/src/asterix/cat048.rs`)
reads every one of the 86 Category 048 blocks; `gungnir-interop/tests/asterix_fixtures.rs`
holds the checks. Three facts about the capture that the first count above missed, each
of which the ingest adapter (GAP-001) has to honour:

- **Categories are interleaved inside datagrams.** 20 of the 100 datagrams carry a
  Category 048 block followed by a Category 034 block. Demultiplexing is per data block,
  not per datagram or per stream.
- **Short frames are padded.** 12 frames are padded to Ethernet's 60-octet minimum; the
  UDP length field, not the frame length, bounds the ASTERIX payload. Padding fed to the
  decoder is a malformed-input error, which is correct.
- **It is a multi-radar feed.** Seven data sources, all SAC 25: SIC 11, 12, 13, 14, 201,
  204, and 205. The 86 blocks hold 128 records; 126 map to detections and 2 are valid
  records that are not observations.

Every record carries I048/010 and I048/140. The one item the build carries without
interpreting in this capture is I048/230 (communications and ACAS capability). No
record uses the reserved expansion or special purpose fields.

The Category 034 decoder built to edition 1.29 (`gungnir-interop/src/asterix/cat034.rs`)
reads all 34 service-message blocks: 32 sector crossings and 2 north markers, from the
same seven radars. 10 of them carry I034/050, every one saying released for operational
use with no overload, a valid time source, and no reset; the other 24 carry only
I034/010, /000, /030, and /020. The standalone `cat034.raw` carries I034/050 (common and
SSR status) and I034/060 (common processing mode) as well.

Through the ingest adapter (`gungnir-ingest/src/adapters/asterix.rs`, 2026-09-06), all
126 detections pass the gateway's validation with every radar bound: the radar clocks run
between 0.5 s and 20 s behind the capture clock and never ahead of it. 48 of the 126
carry a 3D height (I048/110) and map with no recorded loss; the other 78 record theirs.
The 100 datagrams and the two standalone blocks are the `asterix_feed` fuzz corpus in
`gungnir-fuzz/corpus/asterix_feed/`.

## How to use them

- A decoder built to Category 048 edition 1.32 and Category 034 edition 1.29 must read
  every block in these files without error. That is the "reads an actual radar" case
  the synthetic corpora cannot provide.
- A field that decodes from these files but disagrees with the pinned edition is a bug in
  the decoder, not a reading of the capture. The specification governs.
- The `.pcap` needs the Ethernet, IPv4, and UDP headers stripped before the ASTERIX
  payload at byte 42 of each packet; the `.raw` files are bare data blocks.
- Re-verify the hashes above before trusting a copy that has passed through anything
  other than version control.

## Category 205 (`cat205.raw`, added 2026-09-08, GAP-100)

**`cat205.raw` is not a capture. It is a hand-built record, and this section says so
plainly rather than letting it pass for one.** No real-world Category 205 recording
exists to vendor:

- EUROCONTROL publishes no sample recordings for any ASTERIX category, Category 205
  included (`docs/design/external-standards.md` §1.5 already established this for
  Categories 048 and 034; the same is true here).
- The `CroatiaControlLtd/asterix` repository that supplied `cat048.raw` and `cat034.raw`
  carries a Category 205 field **definition** (`install/config/asterix_cat205_1_0.xml`,
  for its own decoder to read) but no file under `asterix/sample_data/` for the
  category: that directory holds only `cat034.raw`, `cat048.raw`, `cat062cat065.raw`,
  `cat_034_048.pcap` and `cat_062_065.pcap`, checked 2026-09-08.
- `asterix-specs` (`docs/design/external-standards.md` §1.4) carries only the edition
  1.0 specification itself in machine-readable form, no sample messages.

So the fixture is synthesized directly from the specification's own byte-layout
tables, the same discipline `docs/design/DN-27-bearing-only-detections.md` and
`docs/design/external-standards.md` apply throughout this workspace: documented,
honest about its origin, and never presented as a real-world capture.

| Field | Value |
|---|---|
| File | `cat205.raw` |
| Bytes | 27 |
| SHA-256 | `342d6750c860984484630d64f44afad40a461e005eca7e34b6e966c80db18305` |
| Origin | Hand-built 2026-09-08 against EUROCONTROL-SPEC-0149-31 edition 1.0 §5, fetched and read in full from `https://www.eurocontrol.int/sites/default/files/2020-03/eurocontrol-cat205p31ed10.pdf` |
| Licence | None -- an original, minimal test value, not a derivative of any third party's data |

**What it holds.** One data block, one record (Category 205 forbids blocking more than
one record per block, edition 1.0 §4.4), a Message Type 5 "Sensor Data Report" with
FSPEC flagging FRN 1, 3, 4, 5, 6, 9, 19, 20, 21:

| Item | Field | Wire count | Decoded value |
|---|---|---|---|
| I205/010 | Data Source Identifier | `0x63 0x01` | SAC 99, SIC 1 |
| I205/000 | Message Type | `0x05` | 5 (Sensor Data Report) |
| I205/030 | Time of Day | `0x54 0x60 0x00` | 43 200.0 s (12:00:00 UTC) -- the same count `gungnir-interop/src/asterix/cat048.rs`'s own hand-built fixture uses for the same time, by construction |
| I205/040 | Report Number | `0x01` | 1 |
| I205/090 | Radio Channel Name | `"121.500"` | `"121.500"` (seven ASCII octets, carried verbatim) |
| I205/070 | Local Bearing | `0x11 0x94` (4500) | 45.00 deg, LSB 0.01 deg |
| I205/180 | Signal Level | `0x15 0x7C` (5500) | 55.00 dBµV, LSB 0.01 |
| I205/190 | Signal Quality | `0xC8` (200) | 200 / 255 |
| I205/200 | Signal Elevation | `0x04 0xE2` (1250) | 12.50 deg, LSB 0.01 deg |

Every count above is the specification's own encoding rule applied by hand (§5.2 of the
primary PDF, cross-checked against `asterix-specs`' cat205 cat-1.0 machine-readable UAP,
which agrees on the field order and lengths in Table 3); "known-correct" here means
"the specification's stated LSB and byte order applied to a chosen count", not a value
taken from any other decoder, since none exists to compare against. The field-by-field
table above **is** the worked reasoning, not a summary of it.

`gungnir-interop/src/asterix/cat205.rs`'s own unit tests build and decode the identical
27 bytes inline (`hand_built_block`), so the crate's fast unit-test coverage and this
on-disk fixture describe the same record; `gungnir-interop/tests/asterix_fixtures.rs`
reads this file with `include_bytes!` for the integration-level "reads a fixture off
disk" case, the same role `cat048.raw`/`cat034.raw` play for their categories.

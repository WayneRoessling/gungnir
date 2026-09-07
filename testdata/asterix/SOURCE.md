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

The structural checks in the last column were made on copy (category byte, declared block
length against file length, pcap magic, block count per category). The edition the
source decoder was written against is not recorded in its repository.

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

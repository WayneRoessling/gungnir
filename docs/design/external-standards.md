# External standards: where the ASTERIX, STANAG 4676, AIS, ADS-B and CoT specifications come from

Status: reference, 2026-09-06. Links verified on that date by fetching each page; the
edition tables below are transcribed from the publishers' pages, not from the PDFs.

**Decisions taken 2026-09-06 by the owner:** Category 048 edition 1.32, Appendix A edition
1.13, and Part I edition 3.1 are pinned (section 1.6). Category 034 **is** decoded
(section 1.7). The GPL-licensed public captures may be used as test fixtures (section 1.5).
Later the same day under D-32: **ITU-R M.1371-6 is pinned for AIS** after the two editions
were compared (section 3.1), the gpsd captures are copied (section 3.3) and the decoder is
built against them (section 3.5); **ICAO Doc 9871 2nd edition with Amendment 2 is pinned
for ADS-B** with no fixture yet (section 4) -- **superseded later the same day**, when the
"no permissively licensed capture" finding turned out to be wrong: two were vendored, the
codec was built and gated against two MIT decoders, and **no normative source is pinned**
at all (sections 4.3 and 4.5). Later still under **D-33**, which put SD-16
in scope for the release: **the CoT event schema version 2.0 is pinned** and the TAK Protocol
Version 1 protobuf framing is deliberately left unpinned until a bearer needs it
(section 5) -- **then pinned on 2026-09-08 under D-33(e)**, when the client's own source
showed that the mesh bearer needs it first (section 5.4).

**What this is.** GAP-064 records that neither the ASTERIX nor the STANAG 4676 specification
is in this repository, and that `AsterixCat048Codec` and `Stanag4676Codec` in
`gungnir-interop` return `NotImplemented` until an engineer has the governing document in
hand (`ARCHITECTURE.md` §10). This note says where each document is, which edition to pin,
and what could and could not be verified. It does not reproduce either specification.
Section 5 does the same job for CoT ahead of GAP-091's codec rather than behind it, which is
the order GAP-064's rule asks for and the first time this repository has managed it.

**What this is not.** No specification has been copied into the repository. The EUROCONTROL
PDFs are public downloads, but the reuse terms are not stated on the download pages and
were not checked, so they stay external and are cited by URL and edition.

## 1. ASTERIX (EUROCONTROL)

Public. Every category specification is a free PDF from EUROCONTROL with no registration.
The root page is <https://www.eurocontrol.int/asterix>; the full list is the library search
at <https://www.eurocontrol.int/library/search?keywords=asterix>.

### 1.1 Part I, the general structure

Read this before any category. It defines the data block, record, FSPEC, and the
encoding rules every category assumes.

| Document | Edition | Date | Download |
|---|---|---|---|
| EUROCONTROL Specification for Surveillance Data Exchange, Part I | 3.1 | 2021-11-23 | <https://www.eurocontrol.int/sites/default/files/2021-11/eurocontrol-specification-asterix-part1-ed-3-1.pdf> |
| List of ASTERIX categories and their statuses | current | 2025-10-22 | <https://www.eurocontrol.int/archive_download/all/node/11440> |

Publication page: <https://www.eurocontrol.int/publication/eurocontrol-specification-surveillance-data-exchange-part-i>.

### 1.2 Category 048, monoradar target reports (Part 4)

Publication page:
<https://www.eurocontrol.int/publication/cat048-eurocontrol-specification-surveillance-data-exchange-asterix-part4>.
Older editions back to 1.10 are listed there too.

| Edition | Published | Download |
|---|---|---|
| **1.32** | 2024-07-02 | <https://www.eurocontrol.int/sites/default/files/2024-07/eurocontrol-cat048-part4-edition-1-32.pdf> |
| 1.31 | 2022-12-15 | <https://www.eurocontrol.int/sites/default/files/2022-12/eurocontrol-cat048-pt4-ed131.pdf> |
| 1.30 | 2021-11-02 | <https://www.eurocontrol.int/sites/default/files/2021-11/eurocontrol-cat048-pt4-ed130.pdf> |
| 1.29 | 2021-08-10 | <https://www.eurocontrol.int/sites/default/files/2021-08/eurocontrol-cat048-pt4-ed129.pdf> |
| 1.28 | 2021-02-22 | <https://www.eurocontrol.int/sites/default/files/2021-02/eurocontrol-cat048-pt4-ed128.pdf> |
| 1.27 | 2020-06-18 | <https://www.eurocontrol.int/sites/default/files/2020-06/eurocontrol-cat048-pt4-ed127.pdf> |
| 1.25 | 2019-08-08 | <https://www.eurocontrol.int/sites/default/files/2019-09/cat-048-09_2019-asterix-spec-0149-4.pdf> |

### 1.3 Category 048 Appendix A, the reserved expansion field

The reserved expansion field (data item I048/RE) is specified separately and versioned
separately. A codec that claims Category 048 must say which appendix edition it decodes.
Publication page:
<https://www.eurocontrol.int/publication/cat048-eurocontrol-specification-surveillance-data-exchange-asterix-part-4-category-48>.

| Edition | Published | Download |
|---|---|---|
| **1.13** | 2024-12-04 | <https://www.eurocontrol.int/sites/default/files/2024-12/eurocontrol-cat-048-appendix-a-p4-ed1-13.pdf> |
| 1.12 | 2024-07-02 | <https://www.eurocontrol.int/sites/default/files/2024-07/eurocontrol-cat-048-appendix-a-p4-ed1-12.pdf> |
| 1.11 | 2022-12-07 | <https://www.eurocontrol.int/sites/default/files/2022-12/eurocontrol-cat048-appendixa-p4ed1.11.pdf> |
| 1.10 | 2022-04-29 | <https://www.eurocontrol.int/sites/default/files/2022-04/eurocontrol-cat048-appendixA-p4ed1.10.pdf> |

### 1.4 Machine-readable definitions

`asterix-specs` (Zoran Bošnjak, BSD-3-Clause) transcribes the EUROCONTROL categories into a
structured source format and generates JSON, HTML, and PDF from it. It carries Category
048 editions 1.27 through 1.32 and Appendix A editions 1.11 through 1.13.

| What | Where |
|---|---|
| Browse | <https://zoranbosnjak.github.io/asterix-specs/specs.html> |
| Source | <https://github.com/zoranbosnjak/asterix-specs> |
| URL pattern, category | `https://zoranbosnjak.github.io/asterix-specs/specs/cat048/cats/cat1.32/definition.json` (also `.ast`, `.txt`, `.pdf`, `.html`) |
| URL pattern, appendix | `https://zoranbosnjak.github.io/asterix-specs/specs/cat048/refs/ref1.13/definition.json` |

Use it to generate field tables or to cross-check a hand-written decoder. It is a
community transcription: where it and the EUROCONTROL PDF disagree, the PDF governs.

### 1.5 Sample data

EUROCONTROL publishes no sample recordings. The only public Category 048 captures found are
in the `CroatiaControlLtd/asterix` decoder repository, which is GPL-2.0 licensed:
`asterix/sample_data/cat048.raw`, `cat034.raw`, and `cat_034_048.pcap` at
<https://github.com/CroatiaControlLtd/asterix>.

**Decided 2026-09-06 by the owner: these captures may be used as test fixtures.** The
conditions that go with that decision, so the fixtures stay defensible:

- The files are test data only. They are never linked into, embedded in, or shipped with a
  binary, so the GPL attaches to nothing the product distributes.
- They live under `testdata/asterix/` with a `SOURCE.md` beside them recording the
  repository URL, the commit the files were taken from, the file paths in that repository,
  the licence, and the date of the copy. A fixture with no recorded origin is not a fixture.
- Decoding the captures is a conformance check against real radar output, not a
  substitute for the specification. A field that decodes from the capture but disagrees
  with edition 1.32 is a bug in the decoder, not a reading of the capture.
- The synthetic corpora GAP-076 seeds from the sample sets under `testdata/` remain the
  fuzz and benchmark input, per D-09. The real captures add the "does it read an actual
  radar" case that the synthetic ones cannot.

**Copied 2026-09-06** to `testdata/asterix/`, at commit `790bca7e`, with the `SOURCE.md`
carrying the commit, the SHA-256 of each file, and what decoding established. The capture
holds 120 data blocks in 100 datagrams, 86 of Category 048 and 34 of Category 034, from
seven radars; 20 datagrams carry a block of each category back to back. That is why
section 1.7's decision matters, and it fixes where the split happens: per data block.

Note that the public capture pairs Category 048 with Category 034 (monoradar service
messages: north marker, sector crossing, status). A real radar feed does the same. The
interop schema registry lists Category 034 alongside 048 since the decision in section
1.7.

### 1.6 What is pinned

**Decided 2026-09-06.** Category 048 edition **1.32**, Appendix A edition **1.13**, and
Part I edition **3.1**: the current editions on that date. They are recorded in the
`AsterixCat048Codec` doc comment, and the conformance tests must name them. Field layouts
change between editions, so a codec without a stated edition is the "confidently wrong"
outcome GAP-064 warns against. Moving to a later edition is a change to this note and to
the doc comment in the same change.

### 1.7 Category 034, monoradar service messages (Part 2b)

**Decided 2026-09-06: yes, Category 034 is decoded.** A monoradar feed interleaves 034
service messages (north marker, sector crossing, radar status, time of day) with 048
target reports in the same data stream, and the sector-crossing and time messages are what
let a receiver order and time-stamp the target reports. Decoding 048 without 034 gives
detections with no per-sector timing and no knowledge of whether the radar is up.

Publication page:
<https://www.eurocontrol.int/publication/cat034-eurocontrol-specification-surveillance-data-exchange-part-2b>.

| Edition | Published | Download |
|---|---|---|
| **1.29** | 2021-03-15 | <https://www.eurocontrol.int/sites/default/files/2021-03/eurocontrol-asterix-cat034-pt2b-ed129.pdf> |
| 1.28 | 2021-03-03 | <https://www.eurocontrol.int/sites/default/files/2021-03/eurocontrol-asterix-cat034-pt2b-ed128.pdf> |
| 1.27 | 2007-05-01 | <https://www.eurocontrol.int/sites/default/files/2019-06/cat034p2bed127.pdf> |

Pinned: edition **1.29**, the current edition on 2026-09-06. `asterix-specs` carries it
at the same URL pattern as section 1.4 with `cat034` in place of `cat048`.

**Design consequence, for GAP-064 to carry.** A service message is not a detection.
`DetectionCodec` returns detections, so an 034 decoder that implemented it would either
return an empty list, which is a silent stub, or smuggle status into detections, which is
the wrong type. The 034 decoder therefore needs its own boundary, producing sensor status
and sector timing for `gungnir-sensor-management` and the time model, and the radar
adapter (GAP-001) demultiplexes the two categories from one stream into the two codecs.
That boundary now exists: `ServiceMessageCodec` in `gungnir-interop`, whose unit is a
`RadarServiceReport` (sensor, source time, the event, the operational status when the
message carried one, the rotation period, the radar's stated position). The Category 034
decoder behind it was built 2026-09-06, the same day as 048; section 1.8 has both.

The capture settled one part of that boundary early: a datagram can carry an 048 block and
an 034 block back to back, so the adapter splits with `gungnir_interop::asterix::data_blocks`
and routes each block by category. The 048 decoder refuses an 034 block by name
(`WrongCategory`) rather than skipping it.

### 1.8 What is built (2026-09-06)

The Category 048 decoder, `gungnir-interop/src/asterix/cat048.rs`, to the pinned editions:

| Layer | What it does | What it does not do |
|---|---|---|
| `asterix::data_blocks`, `Fspec`, `Cursor` | Part I framing: blocks, records, FSPEC, bounds-checked reads that name the failing octet | Interpret any item |
| `cat048::decode_records`, `decode_block` | Every item of the standard UAP (Table 2, FRN 1 to 28) is either typed or carried raw and listed by name in `Record::carried_raw`; an FRN the edition does not define, an undefined compound subfield, or a truncated item is an error at a stated offset | Interpret I048/080, /100, /230, /260, /055, /050, /065, /060, SP, or RE beyond carrying their octets; interpret extension octets of I048/020 and /170 |
| `AsterixCat048Codec::map` and `DetectionCodec::decode` | Attribute a record to a configured `RadarSite` (SAC/SIC to `SensorId` and local-frame origin, supplied by the adapter), place I048/140 on the receipt date, convert slant polar to local ENU using the report's height, and write every loss to `Provenance::conversion_loss` | Attribute a report from an unconfigured radar (`UnknownRadar`); present a `TYP = 0` or position-less record as a detection; encode tracks (`NotImplemented`, by design: Gungnir is the receiving side and a fused track is not a monoradar report) |
| `cat034::decode_records`, `decode_block` (edition 1.29) | Every item of the standard UAP (Table 3, FRN 1 to 14) typed: message type, time, sector and its azimuth, rotation period, system status and processing mode with their compound subfields, message counts, polar window, data filter, WGS 84 position, collimation error; RE and SP carried raw by name | Guess at a spare or extended compound subfield (an error, as for 048) |
| `AsterixCat034Codec::map` and `ServiceMessageCodec::decode` | Attribute a message to a configured `RadarSite`, place I034/030 on the receipt date, and produce a `RadarServiceReport`: north marker or sector crossing (or the other five types), the operational status from I034/050's common part (released, overloaded, time source invalid, track numbers reset), the rotation period, the stated position | Read a missing I034/050 as "released" (status is absent, not assumed); enforce the one-north-marker and thirty-two-sectors promises of §4.3, which belong to the consumer that counts them; carry a strobe's polar window on the report (it stays on the record, and the loss is stated) |

The losses the mapping records, because the specification leaves no other option: height
from Mode C is pressure altitude, not geometric; a report with no height gets the radar's
own height and its slant range as ground range; a report with only I048/042 is the radar's
calculated position, not the measured plot; a report with no time of day takes the receipt
time.

| `gungnir_ingest::adapters::asterix::AsterixFeedAdapter` (GAP-001, radar half, 2026-09-06) | Receive datagrams from a non-blocking UDP socket (unicast or multicast) or a replayed capture, split each into data blocks, route 048 blocks to detections for the gateway and 034 blocks to a service-report queue the host drains, bind SAC/SIC to sensors and put their antennas in the local frame, and count every datagram, block, or record that did not become output under the reason it did not | Hide anything: a malformed datagram, a block that does not decode, an unbound radar, and an unsupported category are each counted and logged, never accepted; a transport failure is an adapter failure and the gateway goes unhealthy. Forward service reports anywhere: the `ProtocolAdapter` boundary carries detections only, and adding a service path to the gateway is a gateway change, which is human-owned |

The adapter's parser is gated on the `asterix_feed` fuzz target in `gungnir-fuzz`, seeded
with the capture's 100 datagrams, and `gungnir-ingest/tests/asterix_seeds.rs` fails if a
seed stops being accepted. `gungnir-ingest/tests/asterix_feed.rs` runs the capture through
the real gateway: 126 accepted, none quarantined, 34 service reports kept apart.

Tests: twenty-three unit tests in the crate (hand-built records of both categories at the
specification's own least significant bits, midnight fold, truncation at every length,
six-bit identification, the 034 compound subfields and WGS 84 position) and seven fixture
tests in `gungnir-interop/tests/asterix_fixtures.rs` against the capture, including
truncation and single-octet corruption of every datagram through both decoders without a
panic. The capture's 34 service messages are 32 sector crossings and 2 north markers.

## 2. STANAG 4676 and AEDP-12 (NATO)

**Not public in any form that could be verified.** STANAG 4676 is the cover agreement; the
technical content is AEDP-12, the NATO ISR Tracking Standard, with AEDP-12.1 as its
implementation guide.

| Document | Edition | Date | Source of the edition data |
|---|---|---|---|
| AEDP-12, NATO Intelligence, Surveillance and Reconnaissance Tracking Standard | Edition A, Version 1 | 2014-05-20 | NISP entry, <https://nisp.nw3.dk/standard/nato-aedp-12-ed.a-v1.html> |
| AEDP-12.1, implementation guide | listed, edition unverified | | GlobalSpec listing |
| STANAG 4676 (cover) | listed, edition unverified | | GlobalSpec, Techstreet, Intertek Inform listings |

What was checked on 2026-09-06 and what came of it:

- The NATO Standardization Office document database at <https://nso.nato.int/nso/> is the
  authoritative source. Every direct URL tried under `nso.nato.int/nso/nsdd/` returned 404
  to an unauthenticated fetch, so no public detail page, classification marking, or
  download link for STANAG 4676 or AEDP-12 could be confirmed. This note therefore makes
  **no claim** about the document's releasability marking. An earlier verbal statement in
  this project that the standard is publicly releasable is withdrawn until someone with an
  NSO account reads the cover page.
- Commercial resellers list AEDP-12 Edition A Version 1 for purchase: GlobalSpec
  (<https://standards.globalspec.com/std/14507144/AEDP-12>), the ANSI webstore
  (<https://webstore.ansi.org/standards/dod/aedp12ed>), Techstreet, and Intertek Inform.
  The ANSI page refused an automated fetch; price and page count were not captured.
- The US DoD ASSIST database (<https://quicksearch.dla.mil/>) accepts a search for
  AEDP-12 but was not queried interactively.
- No newer edition than A/1 (2014) was found. That is absence of evidence from public
  listings, not confirmation that none exists.
- Background reading only, not a specification: the 2008 IET paper "NATO intelligence
  surveillance reconnaissance tracking standard" at
  <https://ieeexplore.ieee.org/document/4567745>.

**Recommended route.** Obtain AEDP-12 Edition A Version 1 and its XML schema through the
NSO database with a registered account, or through the national standardization office,
and read the cover marking before deciding whether the document may sit on a developer
machine. A codec built from the 2008 paper or from a reseller's abstract would be the
outcome GAP-064 exists to prevent. The `Stanag4676Codec` stays `NotImplemented` until
then.

## 3. AIS (ITU-R M.1371 in NMEA 0183 sentences)

**Recorded 2026-09-06 for GAP-010, as candidates; pinned later the same day under D-32
("the agent verifies and pins, the owner reviews").** D-24 said pin both and verify first;
the verification is §3.1's table, the fixtures are §3.3's copy, and the decoder is §3.5.
**For the owner's review**: the pin, the delta reading, and the copies' provenance.

### 3.1 The message payloads

| Document | Edition | Date | Source |
|---|---|---|---|
| ITU-R M.1371, *Technical characteristics for an automatic identification system using time division multiple access in the VHF maritime mobile band* | **M.1371-6** (current) | 02/2026 | ITU, free: <https://www.itu.int/rec/R-REC-M.1371> |
| ITU-R M.1371 | M.1371-5 (previous) | 02/2014 | same |

Both editions are free to download from the ITU. **Pinned 2026-09-06: M.1371-6.** Both
PDFs were fetched (`R-REC-M.1371-6-202602-I` and `R-REC-M.1371-5-201402-S`, the latter
under the ITU's superseded-edition path), extracted to text, and the Annex 7 tables for
the eight message types this system reads were compared field by field:

| Message | -5 | -6 | Delta |
|---|---|---|---|
| 1, 2, 3 (position report) | 168 bits | 168 bits | None in layout. Navigational status 14 reworded ("active AIS-SART, active MOB-AIS or active EPIRB-AIS") |
| 5 (static and voyage data) | 424 | 424 | None in layout. Position-fixing-device enumeration gains **9 = BDS** (-5: 9 to 14 not used) |
| 18 (Class B position) | 168 | 168 | None |
| 19 (extended Class B) | 312 | 312 | None in layout; -6 says future equipment should not send it (24A and 24B instead). Still afloat |
| 21 (aid to navigation) | 272 to 360 | 272 to 360 | None in layout; 9 = BDS in the enumeration |
| 24 part A | 160 | 160 | None |
| 24 part B | 168 | 168 | Bits 166 and 167, spare in -5, become **VDES capabilities** (0 AIS only, 1 VDES ASM, 2 ASM/VDE-TER, 3 ASM/VDE-TER/VDE-SAT). Same offsets; a -5 transmitter sends zero |

So M.1371-6 reads every -5 transmitter unchanged, and the two additions are read as -6
defines them. The codec's doc comment (`gungnir-interop/src/ais/mod.rs`) names the
edition the way `AsterixCat048Codec`'s does, and this table is what a move to a later
edition must be checked against.

### 3.2 The sentence framing

AIS payloads reach a receiver as NMEA 0183 `!AIVDM` (received) and `!AIVDO` (own-ship)
sentences: six-bit ASCII armouring, multi-sentence fragments, a channel letter and a
checksum. NMEA 0183 is IEC 61162-1 and is paid. The public description every open
decoder is written against is the **gpsd project's "AIVDM/AIVDO protocol decoding"**
document, version 1.58, June 2023, by Eric S. Raymond
(<https://gpsd.gitlab.io/gpsd/AIVDM.html>), which tracks ITU-R M.1371 revision 4 and
later. The page states no licence of its own; the gpsd project is BSD-2-Clause. It is a
reference for the framing, not a substitute for M.1371 for the payload fields.

### 3.3 Fixture candidates

| Candidate | What it is | Terms | Reading |
|---|---|---|---|
| gpsd `test/daemon/ais-nmea.log`, `ais-raw-messages.log`, `ais-18-27.log`, `ais-nmea-type6-fid55.log`, `ais_unpack_sixbit.log`, each with a `.chk` file of the expected decode | Raw `!AIVDM` sentences with their decoded form, from gpsd's own regression suite (<https://gitlab.com/gpsd/gpsd/-/tree/master/test/daemon>) | BSD-2-Clause (gpsd) | **Copied 2026-09-06** to `testdata/ais/` at master commit `46977faa`, with `SOURCE.md` recording each file's blob id and the SHA-256 of the copy, and `GPSD-COPYING.txt` beside them. The `.log` files' leading comment blocks named their contributors with e-mail addresses and were removed from the copies; the `.chk` decodes are kept verbatim as the oracle. One legacy reading noted: gpsd decodes the Message 24 vendor id over the pre-2014 42-bit field, so its string runs into the model and serial bits; the test compares the -6 table's three characters as a prefix |
| Danish Maritime Authority AIS data | Historical AIS, free on application | DMA terms | **Not a codec fixture**: the data is decoded CSV, not raw sentences |

### 3.4 What has been verified and what has not

Verified on 2026-09-06: that M.1371-6 exists and is dated 02/2026; that M.1371-5 is
dated 02/2014; that the gpsd document is at version 1.58 (June 2023) and states its
M.1371 basis; that the five gpsd logs above exist in `test/daemon/`. **Verified later the
same day (D-32)**: the layout delta between -5 and -6 for the eight message types
(§3.1's table, from both PDFs); that the gpsd logs carry no licence line of their own and
the repository's `COPYING` (BSD-2-Clause) is what covers them; and every in-scope decoded
value in the `.chk` files, by decoding the sentences independently from the -6 tables and
comparing (`gungnir-interop/tests/ais_fixtures.rs`). **Still not verified**: nothing in
scope; the `.chk` values for message types outside the eight are not read.

### 3.5 What is built (2026-09-06)

`gungnir_interop::ais`: the sentence framing (any talker, six-bit armouring, fragments
joined by sequential id, the checksum) and the eight message types as typed, raw-valued
messages with their sentinels named. Gated on the captures: every in-scope sentence
decodes to gpsd's raw field values. Not an evidence source yet: the receiver adapter and
the local frame are GAP-010's remaining AIS half. **Later the same day**: the receiver
adapter is `gungnir_ingest::adapters::ais::AisReceiverAdapter` (a socket or a recorded
file as the NMEA source), bound per `ais_feeds` entry on both binaries, and the desktop
turns its reports into cooperative evidence for the identification engine; the catalogue
lists it as `ais.m1371` version 1 and the conformance suite decodes the corpus through it.

## 4. ADS-B (Mode S extended squitter)

**Recorded 2026-09-06 for GAP-010; the edition pinned later the same day under D-32.**
The specification of record is **ICAO Doc 9871, 2nd edition (2012) with Amendment 2
(2023)**, which is paid (USD 403, DRM) and not on a developer machine.

**Corrected the same day: the claim that no permissively licensed capture exists was
wrong.** A survey of the open decoder ecosystem found three, all real off-air data:

| Capture | Size | Layer | Licence |
|---|---|---|---|
| `antirez/dump1090` `testfiles/modes1.bin` | 713,736 B, 8-bit IQ at 2 MS/s | RF | **BSD 3-Clause** |
| `rsadsb/adsb_deku` `libadsb_deku/tests/lax-messages.txt` | 215,606 AVR frames, Los Angeles | message | **MIT** |
| `xoolive/rs1090` `crates/rs1090/data/long_flight.csv` | 172,432 Beast frames, one European flight | message | **MIT** |

OpenSky's Zenodo datasets are CC BY 4.0 but hold **decoded state vectors**, which are no
use as codec fixtures; its raw `osky-sample` is GPL-3.0, so copyleft rather than
permissive. **Nobody needs to record their own.**

**No free-and-permissive normative document exists, and one near-miss must be refused by
name.** A DO-260B *draft* circulates publicly as RTCA working paper 1090-WP30-18 carrying
RTCA copyright: free to read, not licensed, and a draft. It is not pinnable and must not
be cited as though it were. FAA TSO-C166c and AC 20-165B are US Government works, so free
and redistributable, but they incorporate DO-260 by reference and carry no bit layouts.
EUROCONTROL is the wrong layer -- ASTERIX Cat 021 is the ground-to-ground target report,
not the airborne message. ITU has nothing on 1090ES.

**So there are two routes and the owner picks one.** Buy Doc 9871 and pin the normative
text; or gate against `rs1090` and `adsb_deku` as MIT dev-dependency oracles over those
captures and pin **no normative source**, recording the residual risk. That risk is real
and specific: the oracles are only partly independent -- `adsb_deku` cites the same ICAO
section numbering, `rs1090` takes inspiration from `pyModeS` -- so a shared misreading
would pass silently, and a codec built that way is bit-compatible with the open-source
consensus rather than verified against the standard.

**Taken 2026-09-06: the second route.** The captures are vendored under `testdata/adsb/`
(the first truncated, and it says so), both oracles are exact-pinned dev-dependencies of
`gungnir-interop` alone, the codec is built, and the parity and the position algorithm are
gated by **arithmetic** rather than by consensus so that the shared-misreading risk has
somewhere it can be caught. Nothing normative is pinned and every place a reader might
mistake the gate for conformance says so. Section 4.5 is what was built; section 4.3 is
the fixtures and what truncating one of them cost.

### 4.1 The specification

| Document | Edition | Date | Source |
|---|---|---|---|
| ICAO Annex 10, *Aeronautical Telecommunications*, Volume IV, *Surveillance and Collision Avoidance Systems* | current amendment unverified | | ICAO, paid |
| ICAO Doc 9871, *Technical Provisions for Mode S Services and Extended Squitter* | **2nd edition (2012), with Amendment 2 (2023)** | 2012, 2023 | ICAO, paid: <https://store.icao.int/> |

Doc 9871 is the authority for the message formats (the "version 0, 1 and 2" registers
of the 1090 MHz extended squitter: airborne position, velocity, identification, and
status). It is paid, and a codec written from it needs the document on a developer
machine; the owner decides whether to buy it or to work from the public reference below
and say so in the codec's doc comment.

### 4.2 Public references and their terms

| Reference | Terms | Reading |
|---|---|---|
| *The 1090MHz Riddle*, 2nd edition, Junzi Sun (TU Delft, 2021), <https://mode-s.org/1090mhz/> | **CC BY-NC-SA 4.0** | The best public description of the formats. **Non-commercial**: usable as reading, not as a source to copy tables or text from into a product. Flagged for the owner |
| pyModeS, <https://github.com/junzis/pyModeS> | GPL-3.0 | A reference decoder to compare against; not a source, and no sample-capture directory was confirmed |
| dump1090 (FlightAware fork), <https://github.com/flightaware/dump1090> | GPL-2.0 | The receiver most deployments run; no test captures visible in the repository |

### 4.3 The fixtures, and what truncating one of them cost

**This subsection said "no permissively licensed raw 1090ES capture was identified", and
that was wrong.** The correction is at the head of section 4 and the two captures were
copied on 2026-09-06 to `testdata/adsb/`, with `SOURCE.md` recording the repository, the
commit, the path, the upstream blob id, the SHA-256 of the copy and what was changed:

| File in `testdata/adsb/` | From | Licence | Layer |
|---|---|---|---|
| `lax-messages-first40000.txt` | `rsadsb/adsb_deku` `libadsb_deku/tests/lax-messages.txt`, commit `a8e4bb2c` | **MIT** | message: AVR frames |
| `modes1.bin` | `antirez/dump1090` `testfiles/modes1.bin`, commit `7ca5a4b3` | **BSD 3-Clause** | radio: 8-bit IQ at 2 MS/s |

**The AVR corpus is truncated and says so.** Upstream is 4.71 MB and 215 606 frames; the
copy is the first 40 000 lines, 882 KB, regenerable with one `head -n 40000`. That loses
four rare type codes — 7 (the file's only surface-position frame), 10, 13 and 22 — twenty-one
frames of 74 641, and `SOURCE.md` tabulates exactly which and what each costs. Two decode
paths are therefore not exercised by real data at all, surface position and the GNSS-height
airborne position, and are gated by the published CPR vectors and by unit tests instead;
the fixture test asserts the surface count is zero so the absence is on the record.

The two candidates this subsection used to recommend are no longer needed and were not
taken: a self-recorded capture (an RTL-SDR near an airport), and the OpenSky Network's
research datasets, whose terms are still unread.

### 4.4 What has been verified and what has not

Verified on 2026-09-06: Doc 9871's edition and amendment; the Riddle's edition and its
licence; pyModeS's and dump1090's licences; the two vendored captures' licences, read in
full from the repositories' own `LICENSE` files at the commits recorded in
`testdata/adsb/SOURCE.md` and copied beside the fixtures. **Not verified**: Annex 10
Volume IV's current amendment; OpenSky's terms; **and, still, that any capture decodes to
what a decoder built from Doc 9871 would produce** — that is precisely the claim the
open-source-consensus route does not make, and section 4.5 says what it makes instead.

### 4.5 What is built (2026-09-06)

`gungnir_interop::adsb`, on the open-source-consensus route the owner approved under
GAP-010. **It pins no normative source and says so in three places** — the module doc
comment, the `SchemaKind::Adsb1090Es { normative_source_pinned: false }` catalogue entry a
peer negotiates against, and the verification-capability-table row's oracle column.

| Layer | What it does | What it does not do |
|---|---|---|
| `adsb::parity`, `parse_avr`, `decode_frame` | Mode S framing: AVR text, the 24-bit parity, the short/long length rule; an extended squitter whose parity does not clear is **refused**, not decoded | Correct an error. dump1090 repairs single-bit errors and this deliberately does not: a repaired frame is a guess |
| `adsb::messages` | Identification (type codes 1–4), surface position (5–8), airborne position barometric (9–18) and GNSS height (20–22), airborne velocity (19), each field carried as transmitted with its own "not available" codes intact | Interpret type codes 0, 23–31: each is **carried with its seven octets and named** (`MeMessage::Carried`), the pattern `asterix::cat048` set with `Record::carried_raw`. Read a DF 18 control field 4 or 7 as an ADS-B message, since neither carries one |
| `adsb::cpr` | Global airborne, global surface against a reference quadrant, local against a reference, and the encoder, plus the closed-form NL expression | Detect a stale reference in a local decode. **The half-cell check every reference decoder carries is dead code in all of them** — the cell is chosen nearest the reference, so the answer is within half a cell of it by construction — so this build omits it and states the caller's obligation instead |
| `adsb::demod` | The pulse-position demodulator, so the BSD-3-Clause IQ capture is a second real recording rather than an unread file: 198 frames whose parity clears without an address, from 0.178 s | Return a surveillance reply whose parity is mixed with the aircraft address. Those are **counted** as candidates it cannot check, never passed on unvalidated |
| `AdsbCodec::stats` | Counts every frame: decoded by group, carried by type code, refused by downlink format, by control field, by parity and by length | Hide anything. Over the vendored corpus the counters and the decodes sum exactly to the 40 000 frames read, and the fixture test asserts that they do |

**Gated three ways, and only one of them is consensus.**
`gungnir-interop/tests/adsb_fixtures.rs` runs all 13 324 extended squitters of the AVR
capture and all 141 demodulated from the IQ capture through this build and through both
oracles, and every field agrees. `adsb_crc.rs` and `adsb_cpr.rs` name **no oracle**: the
parity is checked against the polynomial's own algebra (linearity, every single-bit error,
every burst to 24 bits exhaustively, and the one 25-bit burst it provably cannot see — the
generator itself), and CPR against the worked examples published in `flightaware/dump1090`'s
`cprtests.c`, with the longitude-zone boundaries checked against the inverse of the same
closed form this build evaluates, which agrees with the receivers' transition table to
5e-9 degrees.

**Three oracle defects were found and are recorded rather than smoothed over**, which is
the argument for gating against two decoders rather than one: `adsb_deku` reads seven of
the eight callsign characters, carries the altitude as `u16` and so cannot hold a negative
one, and returns no velocity at all when only the vertical rate is unavailable. None is a
disagreement about a value; every field both oracles produce matches.

**What is still not built.** No adapter owns a receiver, so ADS-B is not yet an evidence
source: turning a position report into a cooperative-identity claim for
`gungnir-identification` needs the same receiver-and-local-frame adapter the AIS half has
(§3.5), and nothing here knows a `SensorId`. Buying Doc 9871 remains a later upgrade that
changes the oracle column and nothing else: the tests are reused unchanged.

## 5. Cursor-on-Target (MITRE)

**Public, and the only format in this note whose releasability is not in doubt.** Pinned
2026-09-06 under D-33, which put SD-16 in scope for the release, and extended 2026-09-07
when the owner asked for the type tree as well, and again 2026-09-08 under D-33(e): **three
artifacts, all three pinned.** The event schema is §5.1, the type tree is §5.7, and the
protobuf framing is §5.4, pinned last and for a reason found in the client's source rather
than in a document. The design that consumes it
is [`DN-25-cursor-on-target.md`](DN-25-cursor-on-target.md); the gaps are GAP-090 and
GAP-091.

### 5.1 The specification of record

The normative artifact is the schema file itself, not a prose guide: `Event-PUBLIC.xsd`, the
CoT event schema, **version 2.0, dated 13 June 2003**, "Copyright (c) 2005 The MITRE
Corporation", carrying the release statement **"Approved for Public Release; Distribution
Unlimited. MITRE Case #11-3895"**. That statement is why this section is short and §2 is
long: unlike STANAG 4676, the marking is on the artifact and needs nobody's account to read.

The version is not a document number anyone looks up. It is the value of the `version`
attribute on every event, and the schema constrains it to a decimal of at least 2, so a
decoder that does not check it is reading an unknown dialect and saying nothing about it.

| Document | Version | Date | Where |
|---|---|---|---|
| CoT event schema, `Event-PUBLIC.xsd` | **2.0** | 2003-06-13 | MITRE, case #11-3895; public transcriptions in §5.3 |
| Cursor-on-Target Message Router User's Guide, MP090284 | -- | 2009 | <https://www.mitre.org/sites/default/files/pdf/09_4937.pdf> -- **refused an automated fetch (HTTP 403) on 2026-09-06**. Background reading, not the artifact the codec is written from |

**Pinned: schema version 2.0.** It is named in the codec's doc comment the way
`AsterixCat048Codec`'s names edition 1.32, and the conformance tests name it too.

### 5.2 What the schema constrains, transcribed 2026-09-06

Transcribed from the public copy in §5.3, because these fields decide what the mapping can
carry and what it must record as lost:

| Element | Attribute | Required | Type as declared |
|---|---|---|---|
| `event` | `version` | yes | decimal, minimum 2 |
| `event` | `uid` | yes | string |
| `event` | `type` | yes | string, pattern `\w+(-\w+)*(;[^;]*)?` |
| `event` | `time`, `start`, `stale` | yes | dateTime |
| `event` | `how` | yes | string, pattern `\w(-\w+)*` |
| `event` | `access`, `qos`, `opex` | no | string; `qos` pattern `\d-\w-\w` |
| `point` | `lat` | yes | decimal, -90 to +90 |
| `point` | `lon` | yes | decimal, -180 to +180 |
| `point` | `hae`, `ce`, `le` | yes | decimal |
| `detail` | -- | no | unconstrained |

Three consequences for DN-25, and they are the reason to transcribe rather than assume:

1. **`ce` and `le` are the entire uncertainty model, and the schema states no confidence
   level for either.** Two decimals in metres, no correlation, no orientation, and nothing
   saying whether they are one sigma or something else. A 6x6 covariance does not survive the
   trip, which is why DN-25 §5 rule 7 records the loss instead of quietly choosing a
   projection -- and why **the mapping must state which multiple of sigma it writes**, since
   the schema will not state it for us. A receiver that assumes the other convention is out
   by a factor no operator can see.
2. **`detail` is unconstrained**, so nothing a partner puts there is schema-checked. Anything
   read out of it is validated by us or not at all: the gateway rule, unchanged, and the
   reason DN-25 puts the inbound feed behind it.
3. **`stale` is mandatory and is the sender's claim about its own data's life.** It is not
   our staleness policy, it does not override it, and DN-16's age rule still decides. A
   sender that stamps an hour does not thereby buy an hour of our trust.

### 5.3 Public copies and their terms

MITRE released the schema as a file rather than through a download page that could be cited
the way EUROCONTROL's can. The copies in public repositories are transcriptions of it:

| Copy | Terms | Reading |
|---|---|---|
| `docjason/XmlValidate`, `schemas/Event.xsd`, <https://github.com/docjason/XmlValidate> | The repository's own; **the file itself carries MITRE's public-release statement and case number** | Used on 2026-09-06 to transcribe §5.2. It preserves the header, the version, the date and the case number, which is what makes it checkable against the released file rather than merely plausible |
| The reference client and server implementations | **GPLv3** (§5.5) | Read, never copied |

Where a transcription and MITRE's released file disagree, **the released file governs** --
the rule §1.4 already sets for `asterix-specs`. Nothing has been copied into this repository.

### 5.4 TAK Protocol Version 1, the protobuf framing -- pinned 2026-09-08

The history is kept, because the reason the pin was refused on 2026-09-06 is the reason it
had to be taken two days later.

A different thing from the schema, and it must not be conflated with it. What was verified on
2026-09-06: a payload is one `atakmap::commoncommo::v1::TakMessage` serialized with protocol
buffers version 3; framing begins with the magic byte `0xbf` followed by the protocol
version, with distinct mesh and stream framings; and negotiation begins in XML, the server
advertising the versions it supports and the client selecting one before either switches.

**No version number and no date exist on that specification.** It is a README and a set of
`.proto` files inside the reference client's source repository, so the only identifier that
could be pinned is a repository and a commit.

**As written on 2026-09-06, superseded by §5.4.1 below.** It is not pinned, and not
needing it is a design property rather than an omission.
Negotiation begins in XML and version 0 (XML) stays accepted, so DN-25's first increment --
codec, inbound feed, multicast sink -- is XML end to end, which is exactly why
[`Sd-Rm.md`](../architecture/uaf/standards/Sd-Rm.md) splits SD-16 across two increments.
**Pinning a commit of a GPLv3 repository as our specification of record is a decision with a
licence question attached** (§5.5), and it is not owed yet. When the stream sink needs it,
the pin is a commit, the question goes to the owner, and this section gains the answer rather
than acquiring one by default.

**Finding 2026-09-08, which undercuts the paragraph above and is recorded rather than
patched over:** the reference client's own source starts a mesh client at protocol version
1 and drops to XML only when a known contact advertises nothing higher than 0, so a stock
ATAK or WinTAK on the multicast group sends protobuf from its first datagram, with no
contacts at all. The first increment's mesh feed and mesh sink therefore meet the framing
this section leaves unpinned, and the recorded corpus will be `takproto-v1` on the group.
The stream connection, by contrast, stays XML until the server advertises version 1. The
evidence, the successor repository the pin would name (`TAK-Product-Center/atak-civ`, tag
5.5.1.8; the repository §5.5 read is archived), and the decision asked of the owner are in
[`tak-interoperability-research.md`](tak-interoperability-research.md) §4 and §7. **Taken
later the same day**, below.

#### 5.4.1 The pin

**Pinned: `TAK-Product-Center/atak-civ`, release tag `5.5.1.8` (2025-10-28), directory
`commoncommo/core/impl/protobuf/`**, under D-33(e). The artifacts are `protocol.txt` (the
framing and the negotiation) and the ten `.proto` files `takmessage`, `takcontrol`,
`cotevent`, `detail`, `contact`, `group`, `precisionlocation`, `status`, `takv` and
`track`, package `atakmap.commoncommo.protobuf.v1`. That is the successor of the archived
repository §5.5 read on 2026-09-06 (archived 2025-05-02, last release 4.6.0.5); the files
are the same in both and neither carries a version, a date or a header of its own, which is
why the pin is a tag and not a document number. The codec's doc comment names the tag beside
the schema version and the guide's case number, and the conformance tests name it too.

What the framing is, from `protocol.txt`, transcribed 2026-09-08:

| Where | Framing | Then |
|---|---|---|
| Mesh datagram | `0xbf` `<varint version>` `0xbf` | one `TakMessage` |
| Stream | `0xbf` `<varint length>` | one `TakMessage` of that length |
| Version 0 | no header; an XML `<event>`, on a stream delimited by `</event>` and prefaced by an XML declaration | -- |

Negotiation on a stream is three XML events: the server advertises
`t-x-takp-v` with `<TakProtocolSupport version="1"/>` at most once per connection, the
client asks with `t-x-takp-q` and `<TakRequest version="1"/>`, the server answers
`t-x-takp-r` with `<TakResponse status="true|false"/>`, and the client waits at least a
minute before giving up. On a mesh every device supporting more than version 0 sends a
`TakControl` at least once a minute, and each broadcasts the highest version every known
contact supports, falling back to 0 when there is no overlap. Version 1's payload is one
`TakMessage` in protocol buffers version 3, and version 1 defines no negotiation
attributes beyond the version number.

#### 5.4.2 The message set, transcribed 2026-09-08 -- what the codec is written from

Under D-33(f) **the codec learns its field numbers from this table and from nothing
copied**: the structs are hand-written against `prost` with these tags, no `.proto` file
enters the repository, and no code is generated from the reference tree. Where this table
and the pinned tag disagree, the tag governs and this table is corrected.

| Message | Field | Tag | Type | Note from the source |
|---|---|---|---|---|
| `TakMessage` | `takControl` | 1 | `TakControl` | optional; "if omitted, continue using last reported control information" |
| | `cotEvent` | 2 | `CotEvent` | optional; "if omitted, no event data in this message" |
| `TakControl` | `minProtoVersion` | 1 | uint32 | 0 reads as 1 |
| | `maxProtoVersion` | 2 | uint32 | 0 reads as 1 |
| | `contactUid` | 3 | string | may be omitted when paired with a `CotEvent` that carries it |
| | `extensionIds` | 4 | repeated uint32 | extensions the sender can *decode*; "must be centrally registered with TPC"; absent means none |
| `CotEvent` | `type` | 1 | string | |
| | `access` | 2 | string | carried as optional; omitted means the CoT value "Undefined"; the comment says MIL-STD-6090 now requires it |
| | `qos` | 3 | string | optional |
| | `opex` | 4 | string | optional |
| | `uid` | 5 | string | |
| | `sendTime` | 6 | uint64 | `time=`, **milliseconds since 1970-01-01T00:00:00Z** |
| | `startTime` | 7 | uint64 | `start=`, same unit |
| | `staleTime` | 8 | uint64 | `stale=`, same unit |
| | `how` | 9 | string | |
| | `lat` | 10 | double | |
| | `lon` | 11 | double | |
| | `hae` | 12 | double | **"use 999999 for unknown"** |
| | `ce` | 13 | double | **"use 999999 for unknown"** |
| | `le` | 14 | double | **"use 999999 for unknown"** |
| | `detail` | 15 | `Detail` | optional; "if omitted, then the cot message had no data under `<detail>`" |
| | `caveat` | 16 | string | optional |
| | `releasableTo` | 17 | string | optional |
| `Detail` | `xmlDetail` | 1 | string | the `<detail>` children not absorbed by the typed fields, serialised in UTF-8 **without** the `<detail>` wrapper and without an XML header; the receiver re-wraps it and parses it as a document |
| | `contact` | 2 | `Contact` | `<contact>` |
| | `group` | 3 | `Group` | `<__group>` |
| | `precisionLocation` | 4 | `PrecisionLocation` | `<precisionlocation>` |
| | `status` | 5 | `Status` | `<status>` |
| | `takv` | 6 | `Takv` | `<takv>` |
| | `track` | 7 | `Track` | `<track>` |
| | `extensionDetails` | 8 | repeated `ExtensionEncodedDetail` | `extensionId` (1, uint32) and `data` (2, bytes); registered extensions only |
| `Contact` | `endpoint` | 1 | string | optional |
| | `callsign` | 2 | string | |
| | `altendpoints` | 3 | string | optional |
| `Group` | `name` | 1 | string | |
| | `role` | 2 | string | |
| `PrecisionLocation` | `geopointsrc` | 1 | string | |
| | `altsrc` | 2 | string | |
| `Status` | `battery` | 1 | uint32 | |
| `Takv` | `device` | 1 | string | |
| | `platform` | 2 | string | |
| | `os` | 3 | string | |
| | `version` | 4 | string | |
| `Track` | `speed` | 1 | double | |
| | `course` | 2 | double | |

Three rules from the source that the codec must keep, because a test written from the
table alone would not catch breaking them:

1. **Whole elements only.** A typed field is populated from a whole `<detail>` child and
   that child is then omitted from `xmlDetail`; a child that appears more times than the
   field allows, or that fails to map, stays in `xmlDetail` whole and the typed field is
   left empty. A receiver that finds the same element in both **keeps the `xmlDetail`
   copy and ignores the typed one**.
2. **The sentinel is not a value.** `999999` in `hae`, `ce` or `le` means unknown. Read
   as a radius it is a thousand-kilometre error, which is a number no gate would refuse
   and no operator would believe; it goes on `Provenance::conversion_loss` as "no error
   stated", exactly as an XML event with no usable `ce` would (DN-25 §5 rule 7).
3. **`staleTime` is the sender's claim, in its unit.** §5.2's consequence 3 is unchanged
   by the unit: DN-16's age rule decides, not the field.

#### 5.4.3 What is verified and what is not

**Verified 2026-09-08** by reading the artifacts: `protocol.txt` and the ten `.proto`
files at tag `5.5.1.8` (an annotated tag, object `deb39afce04c1e0adc1a4aebc411411613654acf`,
pointing at commit `7d583c8834f7f432da8dfc4a1e7c61d7df65e846`, "Release 5.5.1.8
(2025-10-28)"); the
absence of a header on every `.proto` file; the licence file of the repository; and, in
`commoncommo/core/impl/contactmanager.cpp` and `datagramsocketmanagement.cpp` at the same
tag, the initialisation to `SELF_MAX` and the fall-back to 0 that the finding above rests on.

**Not verified:** that the `.proto` files at `5.5.1.8` differ in no way from those at
`4.6.0.5` beyond what a diff would show (they were read at the newer tag only); that
WinTAK, which builds on the same library, behaves as ATAK does on the mesh (inferred);
and iTAK's mesh wire form, which the corpus will show.

### 5.5 Licence

The reference client and the reference server are both **GPLv3**, verified 2026-09-06 by
reading `LICENSE.md` at each upstream repository. The client repository's README adds that
work by US Federal employees may be ineligible for copyright in the United States and that
the licence file governs where it is not.

**Re-verified 2026-09-08 at the successor repository.** The client repository read on
2026-09-06 (`deptofdefense/AndroidTacticalAssaultKit-CIV`) was archived on 2025-05-02 at
release 4.6.0.5; the live one is `TAK-Product-Center/atak-civ`, the same GPLv3 text, the
same README statement, latest release tag 5.5.1.8 with an SDK zip as its asset. The server
(`TAK-Product-Center/Server`) is at 5.7-RELEASE-14. The `.proto` files carry no notice of
their own. **D-33(f), taken the same day, answers the question the second rule below left
open:** the message set is transcribed into §5.4.2 and the codec is written from that
table; no `.proto` file is copied and no code is generated from the tree. This is the
route §5.2 took for the XSD, and it is recorded as the owner's decision, not as a reading
of what the licence permits.

Two rules follow, and they are the two §1.5 already sets for the GPL-licensed captures:

- **The wire format is not the code.** The schema is MITRE's and publicly released; a decoder
  written from §5.2 carries no obligation from the client's licence, which is the same reason
  every commercial sensor vendor emits this format without licensing anything.
- **No file, generated type or fixture is taken from a copyleft repository** except under
  §1.5's recording conditions. That includes `.proto` files: generating Rust types from them
  is touching that tree, which is why §5.4 left the question open until D-33(f) answered it
  on 2026-09-08 by transcription (§5.4.2) rather than by habit.

### 5.6 The corpus: what it must hold, and why it has to be recorded

**The cross-reference this subsection carried is stale, and the correction is worth more
than the citation.** It read "recommended, exactly as for ADS-B (§4.3)". Section 4.3 no
longer recommends a self-recording: two permissively licensed captures were found and
copied on 2026-09-06, on the open-source-consensus route, and that is now the cheaper
answer there.

**That route does not transfer to this format, and the reason should be stated here rather
than discovered halfway through the codec.** ADS-B decodes a *transmitter's* output, and
every open-source ADS-B corpus is a recording of real transmitters, so a permissively
licensed copy is real-world data with a licence attached. The permissive CoT candidates are
recordings of nothing: `pytak` and `takproto` ship **test vectors their authors wrote** to
exercise their own encoders. Gating a decoder on those measures agreement with two Python
libraries' reading of the format, and for ADS-B the equivalent would still have been
evidence about the world. Here it is evidence about a library. So the recording is not a
preference over a cheaper route -- **it is the only route that yields client output at
all** -- and §5.3's reading of the two libraries stands: a cross-check, never the oracle.

#### What the corpus must hold

Six items. Each is here because the mapping, a verification row, or a §5.2 consequence
needs it, and the sixth exists because the corpus is otherwise recorded from one sender.

| # | What to make the client emit | Why the corpus needs it |
|---|---|---|
| 1 | The client's own position, left running for several minutes | The self-report GAP-090 exists for. Several minutes, not several events, because the emission interval, the `stale` horizon and what a client repeats unchanged are all read from the capture rather than assumed |
| 2 | The same client set to **each affiliation it offers** | **This is what turns §5.7's pin from a citation into a check, and it is the item most likely to be skipped.** The type tree is pinned as of 2026-09-07 and `friend` is `^a-f-` -- but that is read from a document published in 2005 and has never been seen on a wire here. Recording a client set to each affiliation is what tests it against the client a deployment will actually meet, and a client that disagrees with the guide is a finding §5.7.5 is waiting for, not a fixture to be discarded |
| 3 | A marker or point placed by hand | The simplest event that is not about the sender, and the one that shows what a client attaches to `detail` when it has little to say |
| 4 | A chat message | §5.2's consequence 2 in the flesh: `detail` is unconstrained, so whatever arrives there is validated by us or by nobody |
| 5 | A deletion, and an event left to expire | `stale` is mandatory (§5.2 consequence 3) and it is the sender's claim, not our policy. A decoder that has never seen an event go stale in a real stream has not met the case |
| 6 | A **second client** on the same group | Two senders on one group is the ordinary deployment, and it is the only way the corpus shows what a mesh sink actually receives. `record_cot.py` says so out loud when it summarises a corpus with one sender |

#### How to record it

The mesh bearer is a UDP multicast group. PyTAK's configuration documentation gives the
client default as `udp+wo://239.2.3.1:6969`, and **the capture is what confirms it** for
the client actually used; the recorder takes `--group` and `--port` for the case where it
does not. Nothing below needs a network, a server, or a certificate: a mesh client and one
host.

1. **A client** -- ATAK on a device or emulator, WinTAK, or iTAK -- on the same host or LAN
   segment, configured for mesh rather than for a server.
2. **`testdata/cot/tools/record_cot.py`**, written for this and standard-library only:

   ```
   python testdata/cot/tools/record_cot.py record --minutes 10
   python testdata/cot/tools/record_cot.py summarize
   ```

   It joins the group, writes every datagram verbatim to `mesh.cotlog` with its receipt
   time in a 24-byte framing header, and writes `mesh.manifest.json` with the per-datagram
   sequence, receipt time, sender, length, SHA-256 and wire form. It **never decodes a
   payload**: a recorder that parsed what it recorded would be a second decoder to keep
   correct, and a wrong one would quietly corrupt the oracle. The wire form is decided by
   the `0xbf` framing byte alone (§5.4), which is how the corpus can report whether the
   client sent XML or protobuf without this repository decoding the latter.
3. **Ten minutes of wall clock**, walking the six items in order and noting the time of
   each, so `SOURCE.md` can say which datagrams are which.

**Expect protobuf on the group (finding 2026-09-08).** §5.4's finding applies here: a
stock client on the mesh sends `takproto-v1`, so the manifest's `wire` column will say so
for every datagram unless an XML-only client is on the group. The XML half of the corpus
comes from the same client over a TCP "server" connection to a recorder that sends nothing:
`record --tcp` (default port 8087, the plain streaming port a client offers when a server
is added with SSL off), which writes `stream.cotlog` and `stream.manifest.json`, splits
the byte stream on `</event>` exactly as the reference client's `takproto/README.txt` says
clients delimit it, and would frame a `0xbf`-prefixed message by its varint length if one
ever arrived, which it should not, since the recorder never advertises.
[`tak-interoperability-research.md`](tak-interoperability-research.md) §6 says why the
corpus has two halves. An Android emulator cannot reach the group at all and needs that
mode or `record --unicast`, which binds the UDP port without joining, for a client sending
to `10.0.2.2` (§3 there). The walk of the six items is done once per half, with the
client's server connection switched on for the stream half.

The recorder was exercised against loopback multicast on 2026-09-07 with synthetic
datagrams that were then deleted: it joined the group, framed four datagrams, distinguished
the two wire forms, read the log back, and reported the single sender. **That test proves
the recorder, not the format.** No CoT has been recorded.

#### What `SOURCE.md` must record

The same discipline as `testdata/ais/` and `testdata/adsb/`, with the fields a recording
has that a copy does not:

- The **client and its exact version**, the platform it ran on, and the date. A capture
  whose client version is unknown cannot be re-read later when a version changes behaviour.
- The **group, port and interface**, and whether the group was loopback-scoped or on a LAN.
- Which datagram ranges are which of the six items, by sequence number.
- The **SHA-256 of `mesh.cotlog`**, the datagram count, the payload byte count, the wire
  forms and the senders -- all of which `summarize` prints, so they are read back from the
  file rather than typed from memory.
- Whether sender addresses were stripped, and if so, why.
- **What the corpus does not contain**, in the manner of `testdata/adsb/SOURCE.md`'s
  account of its truncation: any of the six items that could not be produced, named, with
  what each costs.

#### The rule that makes it a fixture

**Recorded, never authored.** Nothing under `testdata/cot/` may be hand-written CoT, and
the recorder has no mode that would write any. A corpus authored by the same hand that
writes the decoder passes against itself and fails against every real client, which is the
whole of GAP-064's rule and the reason §1.5's conditions are worded around *origin* rather
than around content. If the recording cannot be made, the directory stays as §5.7 leaves
it and the codec waits.

#### Candidates that are not the corpus

| Candidate | What it is | Terms | Reading |
|---|---|---|---|
| `snstac/pytak` test data, <https://github.com/snstac/pytak> | Python TAK integration library | **Apache-2.0** | Copyable under §1.5's recording conditions, and useful: a second reading of the format to disagree with. Author-written rather than client-emitted, so it is a cross-check and never evidence about what a client sends |
| `snstac/takproto` test data, <https://github.com/snstac/takproto> | Encoder and decoder for the protobuf payloads | **MIT** | The same reading; relevant now that §5.4 is pinned, as a cross-check on the wire bytes of the mesh half. Its vendored `.proto` files are the reference tree's under another name, so they change nothing about origin |
| The reference client and server repositories | The implementations themselves | **GPLv3** (§5.5) | Not a fixture source. §5.5's second rule covers their test data as well as their code |

**Nothing has been copied and nothing has been recorded.** GAP-091's closing action puts
the corpus before the codec, in that order, for the reason §1.5 gives: a fixture with no
recorded origin is not a fixture.

### 5.7 The type tree -- pinned 2026-09-07

§5.6 item 2 named this as the gap in §5.1's pin: the event schema says the `type` attribute
must match `\w+(-\w+)*(;[^;]*)?` and says nothing whatever about what it means, so DN-25 §5
rule 3 -- only `Friendly` is acted on -- had no pinned definition of which type strings are
friendly. This subsection closes that as far as public artifacts allow, and says plainly
where they stop.

#### 5.7.1 The specification of record

| Document | Edition | Date | Release statement |
|---|---|---|---|
| **The Developer's Guide to Cursor on Target**, MITRE Technical Report, Mike Butler, Center for Air Force Command & Control, Bedford, Massachusetts | the August 2005 report | 2005-08 | "©2005 The MITRE Corporation. All Rights Reserved." / **"Approved for Public Release; Distribution Unlimited. Case #06-0249"** |

**Pinned: the August 2005 guide, MITRE case #06-0249**, for the grammar and the semantics
of `type`. It is named in the codec's doc comment beside the schema version, and the
conformance tests name it too.

Obtained 2026-09-07 by fetching a public copy and reading the document itself, not a
summary of it: twenty pages, the cover carrying the title, the date, the author, the MITRE
copyright and the release case number quoted above. MITRE's own host refused an automated
fetch of the companion Message Router guide on 2026-09-06 (§5.1) and was not retried; the
release marking is on the document, which is what matters, and §5.7.5 says what that does
and does not establish.

#### 5.7.2 What the guide settles, transcribed 2026-09-07

Six findings. Each is here because a decoder that missed it would be wrong in a way its own
tests would not catch.

1. **The type is a path through an object hierarchy, not an enumeration.** `a-h-G-E-V-A-T-t`
   is `atoms::hostile::ground::equipment::vehicle::armored::tank::t72`; the hyphens separate
   branches so that the tree can have cardinality greater than the alphabet. **Partial
   understanding is the design**: the guide states that a receiver knowing only the first
   three branches still reads `a-h-G-E-V-A-T` as `a-h-G-<something>`, "a hostile ground
   unit". So a decoder that refuses a type it cannot resolve in full is refusing exactly
   what the format was built to let it accept, and this note's mapping reads a prefix on
   purpose rather than for want of a table.
2. **The root branches, with a defect in the guide's own list.** As printed: `a` atoms
   ("anything you drop on your foot"), `b` bits ("a chunk of information, e.g., image or
   chat"), `t` tasking, `r` reply, `c` capability, `r` reservation. **`r` is assigned
   twice**, to reply and to reservation. Nothing here resolves it and nothing here needs to:
   DN-25 touches neither branch. A later build that needs one settles it against the type
   file (§5.7.4) and records what it found, rather than picking the one that suits it.
3. **Affiliation lives in the type, and only in the atoms branch.** The guide's own
   self-criticism, in its list of warts: "Affiliation (friend, hostile, ...) is in the type.
   That's not the ideal place, but it is the best place. It's only applicable to the atoms
   branch." **Reading affiliation by position is therefore wrong**: a `b-` event -- a chat
   message, an image -- has no affiliation at all, and a decoder that takes the second
   element of every type as an affiliation will invent one for it.
4. **The atoms subtree is MIL-STD-2525B, pruned.** "The Atoms tree is the most populated, it
   is based on MS2525B. (This is a change from V1.0 which used our own organization.)" It
   inherits that standard's redundancies knowingly -- "MS2525B has some redundancies. For
   example, there are multiple representations for helicopter. This is a wart we adopted" --
   and MITRE has "pruned some branches of that tree and intend to prune more". **So pinning
   MIL-STD-2525 would be the wrong pin.** CoT's tree is neither 2525B nor any current
   edition of 2525; 2525B is its ancestor, and the tree is the artifact.
5. **Case is significant, and the schema does not say so.** From the type file's own header:
   "Upper case strings are taken directly from the mil-std-2525 type hierarchy, lower case
   characters are CoT extensions. The matching *is* case sensitive!!!" §5.2's transcribed
   pattern permits both cases and cannot express which means what, so **a decoder that
   case-folds a type string is wrong** and would silently conflate a 2525 branch with a CoT
   extension.
6. **The predicate, not the prefix, is the recommended test.** "As with other 'magic
   constants,' it's unwise to hard-code these class strings into your code." The recommended
   form is a file of predicates, each `<is what="..." match="..."/>` with a Perl-style
   regular expression, evaluated as `if(event->is("friend"))`, and the guide calls it "the
   recommended way to make runtime type decisions with CoT".

#### 5.7.3 The one predicate this system needs

DN-25 §5 rule 3 asks one question of the type: is this event friendly. The guide publishes
that predicate verbatim, as its own worked example of the mechanism:

```xml
<is what="friend" match="^a-f-" />
```

**Pinned: `friend` is `^a-f-`, case-sensitive, from the August 2005 guide.**

What the pin buys, and what it deliberately does not:

- **It answers the atoms question and the affiliation question in one anchored test.**
  `b-t-f-...` does not match, which is the right answer by finding 3 -- a chat message has
  no affiliation -- where a positional reading would have manufactured one.
- **It is case-sensitive by finding 5.** `A-F-` is not a friendly and is not folded into
  one.
- **It licenses nothing else.** This note pins `friend` and no other affiliation, because
  rule 3 acts on `Friendly` alone and everything else is recorded and does nothing. Writing
  down the other letters from memory is precisely the failure this whole section exists to
  prevent, and the corpus is what would earn them (§5.6 item 2).

#### 5.7.4 Two files called `CoTtypes.xml`, and only one is obtainable

Confusing them would be the easy mistake, so both are named:

| File | What it holds | Obtainable |
|---|---|---|
| **The predicate file** the guide recommends | `<is what="..." match="..."/>` entries | **No.** "The sole 'official' CoT types tree is in CoTtypes.xml which ships with the CoT debugger", and the debugger is not public. §5.7.3 takes the one predicate it needs from the guide's own text instead of from a file nobody can fetch |
| **The type mapping file** | Root `<types>`, roughly nine hundred `<cot cot= full= desc=/>` entries mapping CoT types to other message formats' types | **Yes, permissively.** `dB-SPL/cot-types` (**Apache-2.0**), <https://github.com/dB-SPL/cot-types>, copied there from ESRI's `solutions-geoevent-java`. Header: `$Id: CoTtypes.xml,v 1.80 2009/06/10 17:01:00 econnors Exp $`, `Copyright (C) 2003 MITRE Corporation`, author Mike Butler, dated 02-Mar-03 |

**Revision 1.80 (2009-06-10) is the version to name** should a build ever need the full
tree. Use it the way §1.4 says to use `asterix-specs`: to generate tables or to cross-check
a hand-written decoder, never as the authority, and where it and the guide disagree the
guide governs. Its header also carries finding 5 and one more that matters -- the mappings
exclude the affiliation prefix "to allow the same type mapping to be used for all variants
(`a-h-`, `a-f-`, `a-u-`, ...)", which is the file's own confirmation that affiliation
separates cleanly from the rest of the path.

**Nothing has been copied into this repository.** DN-25 needs one predicate, §5.7.3 has it,
and copying nine hundred entries to use one of them would invert the rule §5.4 follows: pin
what is needed, name what is not.

#### 5.7.5 What is verified and what is not

**Verified 2026-09-07**, by reading the artifacts rather than descriptions of them: the
guide's title, date, author, publisher, copyright line and public-release case number; each
of the six findings in §5.7.2, quoted from the document; the text of the `friend` predicate;
and the mapping file's header line, revision, date, author and licence.

**Not verified**: that the predicate file's current `friend` entry still reads `^a-f-`, the
file being unobtainable and the guide being twenty years old; that revision 1.80 is the
latest of the mapping file; whether MITRE has published a later edition of the guide; and
the `r` collision in finding 2, which is recorded rather than resolved.

**This is why §5.6 item 2 stays on the corpus list even now that the mapping is pinned.** A
pin read from a 2005 document and never seen on a wire is a pin, not evidence. Recording a
client set to each affiliation is what turns `^a-f-` from a citation into a check, and if a
real client disagrees with the guide, the capture is the finding and this subsection gains
it.

### 5.8 What is built

**No codec, and no corpus; all three artifacts pinned as of 2026-09-08.** `gungnir-interop` has no CoT codec, the schema catalogue has
no entry for it, and no binary opens a socket for it. GAP-090 and GAP-091 are open. This
section exists so that the codec, when it is written, can name what it decodes -- GAP-064's
rule, which is the rule this whole note was written to serve.

**One thing is built, and it is not a decoder.** `testdata/cot/tools/record_cot.py` is the
recorder §5.6 specifies, standard-library only, exercised against loopback multicast on
2026-09-07 and holding no data. **On 2026-09-08 it gained the `--tcp` and `--unicast`
modes** that give the corpus its stream half and its emulator route, exercised against
loopback the same way, with the synthetic bytes deleted afterwards, and still holding
nothing. `testdata/cot/SOURCE.md` says in its first line that the
directory is empty and why, because an empty fixture directory with no note beside it reads
as an oversight rather than as a state. Recording the corpus needs a TAK client, which is
the one thing this workspace cannot supply itself -- the same shape of blocker as §4's ADS-B
capture before the open-source route replaced it, and §5.6 explains why that route does not
replace this one.

## 7. SAPIENT (UK Dstl; NATO STANREC 4869) -- **pinned 2026-09-06**

**The specification of record for the acoustic, passive-RF and spotter feeds is the
SAPIENT Interface Control Document v7, DSTL/PUB145591, dated 2023-02-01**, with the wire
format taken from the v2.0 protobuf schemas at `github.com/dstl/SAPIENT-Proto-Files` and
the fixtures from `github.com/dstl/Apex-SAPIENT-Middleware`. The ICD is the pinned text
rather than BSI Flex 335 for one reason and it is a licence one: **the ICD is Open
Government Licence v3.0 and may be vendored; BSI Flex 335, which is the current normative
version, is free to read and may not be.** Where the two differ the BSI version governs,
and this note must be revisited if a difference is found -- which is a real risk and is
recorded here rather than assumed away.

**Why this section exists.** GAP-001 and GAP-004 recorded the acoustic, RF and spotter
feeds as "blocked on interface agreements nobody has". That was wrong. A single free
document specifies all three, and its schemas and sample messages are Apache-2.0.

SAPIENT is the UK Ministry of Defence open sensor-network interface, adopted by NATO as
**STANREC 4869 / AEDP-4869** with STANAG ratification in progress, and exercised at NATO
counter-UAS trials with seventy-odd vendor connections.

### 7.1 What is free, and what is only free to read

The distinction matters more here than anywhere else in this document, because the
normative version and the redistributable version are different artefacts.

| Artefact | Where | Free to obtain | Redistributable |
|---|---|---|---|
| **SAPIENT Interface Control Document v7**, DSTL/PUB145591, 2023-02-01, 73 pp | `https://assets.publishing.service.gov.uk/media/6419a2068fa8f547c68029d3/SAPIENT_Interface_Control_Document_v7_FINAL__fixed2_.pdf` | Yes, direct, no account | **Yes -- Open Government Licence v3.0**, Crown copyright, stated on page 1 |
| **BSI Flex 335 v2.0:2024-03**, the current normative version | BSI Knowledge, linked from `https://www.gov.uk/guidance/sapient-autonomous-sensor-system` | Yes, at no charge, account required | **No** -- BSI retains copyright |
| Protobuf schemas, v1.0 and v2.0 | `https://github.com/dstl/SAPIENT-Proto-Files` | Yes | **Yes -- Apache-2.0**, Crown copyright |
| Sample messages | `https://github.com/dstl/Apex-SAPIENT-Middleware`, `tests/resources/` | Yes | **Yes -- Apache-2.0** |
| Test harness | `https://github.com/dstl/BSI-Flex-335-v2-Test-Harness` | Yes | Apache-2.0 |

**So the "pinned specification plus a public sample in the repository" rule is satisfiable
without a single agreement**: the ICD under the Open Government Licence for the normative
text, the Apache-2.0 protobufs for the wire format, and the Apache-2.0 sample messages as
fixtures. The ICD's §7 also carries worked examples for every message type, under the same
licence.

### 7.2 Why it fits three of the four feeds

The node taxonomy names them, verbatim from `registration.proto` v2.0:

```text
NODE_TYPE_ACOUSTIC   = 6;   // a microphone or an acoustic array
NODE_TYPE_PASSIVE_RF = 8;   // passive interception of radio-frequency signals
NODE_TYPE_HUMAN      = 9;   // a human acting as part of the system, such as a spotter
```

`DetectionReport` reports **bearing-only** natively: `RangeBearing` makes `azimuth`,
`elevation` and `range` each optional with a paired error field and a datum enumeration,
which is the shape an acoustic array and a direction finder actually produce and the shape
this workspace's `DetectionView` does not currently have. It carries a `Signal` repeated
field with amplitude and start, centre and stop frequency for the RF case, a three-level
classification tree whose taxonomy includes weapons and emitters, a behaviour enumeration,
and `associated_file` URLs for an audio clip or a photograph.

**A spotter therefore needs no design of our own**: a person's application registers as a
`HUMAN` edge node and emits ordinary detection reports.

### 7.3 What this does not settle

The bearing-only shape is the real work. `gungnir_model::DetectionView` carries a
position; a detection with a bearing and no range is not one, and forcing it into a
position by inventing a range is exactly the confidently-wrong answer this system's rules
forbid. **That is a design note, not an adapter**, and it is the thing to write before any
of the three feeds is built.

Drone Remote ID is the one sub-case that costs money: ASTM F3411 and ASD-STAN prEN
4709-002 are both paid, of the order of seventy dollars. That is a standards fee and not an
interface agreement, and `github.com/opendroneid/specs` holds only early drafts and says so.

NATO ACCS ASTERIX categories 158 (strobe reports) and 160 (passive sensor data) are exactly
on topic and are **not** available from the public ASTERIX site.

## 8. Motion imagery: STANAG 4609 and the MISB family -- surveyed 2026-09-06, the metadata half built 2026-09-08 against a secondary source, **still not pinned to the primary text**

**Still not pinned to MISB's own text**, and §8.2 says exactly why: the specification is
free but the bot gateway below defeated every automated attempt to fetch it, this session
included. What changed 2026-09-08 (GAP-099) is that the fixture decision §8.1 left open is
made, and the metadata half of the feed -- platform position, orientation and sensor
pointing, never the video itself -- is built and gated against it, with its tag semantics
read from a permissively licensed secondary implementation rather than from MISB's text.
That is a real decoder with a stated, checkable provenance, and it is not a pin: a pin is
what happens when this workspace's own reading of the primary document is recorded, and
that has still not occurred.

**Free, public, and needing no account.** The live registry is the NSG Standards Registry;
the old `gwg.nga.mil/misb/...` paths are dead and must not be cited.

| Document | Stable citation | Note |
|---|---|---|
| MISB ST 0601.19, *UAS Datalink Local Set*, 2023-03-02 | `https://nsgreg.nga.mil/doc/view?i=5471` | The metadata set itself |
| MISB ST 0102.12, *Security Metadata* | `https://nsgreg.nga.mil/doc/view?i=4422` | Carries the classification markings |
| MISB ST 1402.2, *MPEG-2 Transport Stream* | `https://nsgreg.nga.mil/doc/view?i=4273` | How the metadata rides the video |
| MISB ST 0807.27, *KLV Metadata Registry* | `https://nsgreg.nga.mil/doc/view?i=5680` | The tag registry |
| MISB ST 0805.1, *KLV to Cursor-on-Target Conversions*, 2014-02-27 | `https://nsgreg.nga.mil/doc/view?i=4161` | A free, NGA-published definition of CoT fields, useful to §5 |
| MISP-2025.1, the profile | `https://nsgreg.nga.mil/doc/view?i=5743` | Mandated in the US DISR |

**STANAG 4609 itself is public**: NSO record `https://nso.nato.int/nso/nsdd/main/standards/stanag-details/9257/EN` shows Edition 5, promulgated 2020-07-30, security class NON CLASSIFIED, downloadable without a login. It is a thin covering agreement whose technical content is the MISP.

**On licence, be precise.** ST 0601.19 and MISP-2025.1 as published today carry **no
printed distribution statement**; older editions carry "DISTRIBUTION A". The honest
characterisation is that these are US Government works on a public unauthenticated
registry: free to read and download, no copyright asserted, and **no explicit licence grant
either**.

**A practical warning for whoever writes the pin.** Both NGA sites sit behind a bot
gateway, so scripted fetches receive a challenge page while a browser session does not. Any
tooling that checks these links must expect that.

### 8.1 The sample is the gap, not the specification

| Candidate | Licence | Verdict |
|---|---|---|
| `samples.ffmpeg.org/MPEG2/mpegts-klv/` "Day Flight" and "Night Flight IR" | **None stated anywhere** -- no README, no note; FFmpeg's own licence covers code, not samples | **Do not vendor.** Free to fetch, provenance undocumented |
| `github.com/paretech/klvdata`, `data/DynamicConstantMISMMSPacketData.bin` | **MIT** | Redistributable; derived from the ST 0601 worked example. Best available |
| `github.com/WestRidgeSystems/jmisb` | **MIT** | Redistributable; its examples generate synthetic video and metadata |
| MISB's own exemplar and conformance files | APAN account required | Not public |

So the motion-imagery adapter is blocked on **a fixture decision**, not on a specification
and not on an agreement: take the MIT binary, or synthesise a stream from the worked
examples in ST 0601.

**Decided 2026-09-08: the MIT binary.** `paretech/klvdata`'s worked example over
`jmisb`'s synthetic generator, because a decode fixture needs to stay byte-identical
across runs and a static file is the more direct fit than something a library
generates fresh; copied to `testdata/misb/` with `SOURCE.md` on the terms §1.5 sets.

### 8.2 What is built (2026-09-08)

The UAS Datalink Local Set decoder, `gungnir-interop/src/misb0601/mod.rs`, for GAP-099.

**What is and is not pinned, stated once more because it is the point of this
section.** The 16-byte Universal Label and the generic BER short/long-form
tag-length-value framing are independent public knowledge, verified from multiple
sources that are not MISB. The seventeen tags this decoder interprets -- their
numbers, and the domain/range a "mapped" numeric field linearly scales between -- are
transcribed from `klvdata/misb0601.py` (`github.com/paretech/klvdata`, MIT, commit
`79028b4ab4ce7192d1b7c04d2266fc31ac337511`), a secondary source, not from MISB's text,
which the bot gateway above refused to serve to a scripted fetch again this session.
Every decoded value this decoder produces for the vendored fixture was checked against
klvdata's own Python, *run* against the identical bytes rather than read and
paraphrased, and that run's output is the oracle `gungnir-interop/tests/
misb0601_fixtures.rs` gates on. A future session that reaches ST 0601.19 itself (a
browser session past the gateway, or a printed copy) should diff this tag table
against it the way §1.6 diffed `asterix-specs` against the EUROCONTROL PDF; the
secondary source governs nothing once the primary text is in hand.

| Layer | What it does | What it does not do |
|---|---|---|
| `misb0601::decode_frame`, `read_ber_length`, `find_next_key` | The generic KLV framing: the 16-byte key, BER short/long-form length, bounds-checked reads that name what is missing rather than panicking, and a resynchronization search for a stream that has lost alignment | Assume a datagram boundary lines up with a frame boundary -- KLV rides an elementary stream, so a frame may arrive split across reads, which the adapter's own buffer resolves |
| `misb0601::apply_tag`, the per-tag `mapped`/`as_string` helpers | Seventeen tags: Checksum, Precision Time Stamp, Mission ID, Platform Tail Number, Platform Heading/Pitch/Roll, Platform Designation, Image Source Sensor, Sensor Latitude/Longitude/True Altitude, Sensor Relative Azimuth/Elevation/Roll, Slant Range, Frame Center Latitude/Longitude/Elevation, and the LS Version Number, each cited by tag number in `Misb0601Frame`'s field docs | Interpret any other tag -- MISB ST 0601 defines upwards of ninety -- or a known tag encoded at a width outside 1, 2 or 4 bytes; both are carried in `Misb0601Frame::carried_raw` by tag number and raw bytes, never dropped and never guessed at. Interpret the nested ST 0102 Security Local Set (tag 48) |
| `packet_checksum` | MISB ST 0601's additive checksum algorithm, reconstructed from `klvdata.common.packet_checksum` and confirmed against the maintainer's own description of its contract (`paretech/klvdata` issue 7, quoting MISB ST 0601.8-08's discard rule) | Decide whether a mismatched frame is discarded -- that is `gungnir_ingest::adapters::misb`'s job, not the codec's, matching how this workspace separates decoding a message from deciding to trust it |
| `gungnir_model::UasPlatformReport`, `EnuPoint` | A platform's position, heading/pitch/roll, sensor-relative pointing, slant range, frame-centre ground point, and its free-text identity fields, as a report type in `gungnir-model` -- the report-shaped counterpart to AIS's and ADS-B's `CooperativeReport`, placed here rather than beside its adapter because its shape is closer to a `Measurement::Bearing`-style report than to a single identity claim | Assert that any of it is true. Nothing at the ingest boundary verifies a platform designation or an orientation angle any more than AIS verifies a vessel name |
| `gungnir_ingest::adapters::misb::UasMetadataAdapter` (GAP-099, 2026-09-08) | Buffer a KLV byte stream from a TCP source or a recording, decode complete frames off the front, place a fix (Sensor Latitude/Longitude/[Altitude]) in the local ENU frame and emit it as a `DetectionView` through the real gateway exactly as every other feed's position does, hand every accepted frame's full report to a side-channel sink, and enforce MISB ST 0601.8-08's checksum-discard rule (a mismatched frame produces neither a detection nor a report, counted by name) | Decode, transport, or display the video itself -- out of scope by the design survey's own separation of a video transport from a detection message. Bind a live feed in either binary: no `misb_feeds` configuration entry exists yet in `gungnir-config`, so this is built and gated but not wired, the same distinction GAP-001's own history draws between its acoustic/passive-RF adapters being built and later being wired |

**A genuine finding, not a guess, recorded rather than smoothed over**: the vendored
fixture's own stated checksum (`0xAA43`) does not match what `packet_checksum` computes
over its preceding bytes (`0x3E1E`). Every plausible alternative byte range was tried
and none closes the gap, consistent with the fixture's own upstream test-suite comment
that some errors in transcribing the original MISB worked example "may have been hand
corrected". The decoder decodes the frame's fields regardless, since KLV framing does
not depend on the checksum, and reports the mismatch on `Misb0601Frame::checksum_valid`
rather than silently trusting or silently refusing a real, MIT-licensed worked example.

`gungnir-sensor-management`'s node-type taxonomy needed no change for this: unlike
SAPIENT's `registration.proto` node types (§7.2), `SensorRecord.modality` is a
free-text `String`, so a deployment names this feed's modality in its own
configuration without a code change here.

Tests: 12 unit tests in `gungnir-interop::misb0601` (BER length forms, the linear-map
arithmetic against the fixture's own heading bytes, an error sentinel read as absent,
the checksum algorithm against hand-computed sums, a self-consistent hand-built frame,
an unknown tag carried raw, truncation asking for more rather than erroring, and
key-mismatch resynchronization) and 4 fixture tests in `gungnir-interop/tests/
misb0601_fixtures.rs` against the vendored capture, gated on klvdata's own reading of
the identical bytes and on the checksum mismatch itself. 4 unit tests on the adapter in
`gungnir-ingest` and 2 tests running the vendored fixture and a hand-built well-formed
frame through the real `IngestGateway` end to end (`gungnir-ingest/tests/misb_feed.rs`).

## 9. Radio direction finding: ASTERIX Category 205 -- surveyed 2026-09-06, **pinned and built 2026-09-08 (GAP-100)**

**Surveyed as the cheaper option passed over on purpose, then taken.** The 2026-09-06
survey found Category 205 nearly free to add given the existing ASTERIX decoder, but left
it unpinned because it advances one feed where §7's SAPIENT pin advances three. GAP-100
took it up 2026-09-08: the bearing-only decision this section always said it needed
(§7.3, DN-27) was confirmed built and unchanged on `main` first, per that gap's own
instruction to check rather than assume before writing a second bearing representation.

The cheapest of all of these, because §1 already pins documents from the same family under
the same terms.

### 9.1 What is pinned

**EUROCONTROL-SPEC-0149-31, ASTERIX Part 31, Category 205, Radio Direction Finder
Reports**, Edition **1.0**, 2020-03-17, 41 pp, ISBN 978-2-87497-028-3. The document's own
status page reads "Released Issue", "Intended for: General Public", "Accessible via:
Internet". Free PDF at
`https://www.eurocontrol.int/sites/default/files/2020-03/eurocontrol-cat205p31ed10.pdf`,
fetched and read in full for GAP-100 rather than assumed from this survey's own summary.

Its data items are a direction-finder report and nothing else: `I205/070` local bearing,
`I205/080` system bearing, `I205/090` radio channel name, `I205/100` quality of
measurement, `I205/110` estimated uncertainty, `I205/120` contributing sensors, and both
geodetic and Cartesian position. Adjacent and also free: **Category 129, UAS
Identification Reports**, Edition 1.2, 2019-06-12 -- surveyed the same day as 205 but
**not pinned and not built under GAP-100**: it is a different report shape
(identification, not a bearing) that would not share Category 205's one hard design
question (9.2 below), and forcing it into the same change would not have simplified
verifying that question's answer. It remains open for a future gap.

**One nuance this pin has that Category 048 and 034's did not.** Edition 1.0 itself cites
Part I edition **2.4** (24 October 2016) in its own bibliography, not the edition 3.1
§1.6 pinned and `gungnir-interop`'s shared ASTERIX framing already implements. The two
editions agree on the data block, record and FSPEC structure that framing depends on,
checked by hand against edition 1.0's own §4.4 diagram; a difference elsewhere in Part I
between 2.4 and 3.1 would not be caught by that comparison and has not been separately
checked. Cross-checked against `asterix-specs`' cat205 edition-1.0 machine-readable UAP
per §1.4's own rule (to cross-check, never as the authority): the two agree on field order
and lengths.

**Nothing new has to be licensed to use it.** What was needed was the same bearing-only
decision §7.3 names, and it was already built and signed (DN-27) before this section's
survey was written.

### 9.2 What is built (2026-09-08)

`gungnir-interop/src/asterix/cat205.rs` decodes every standard-UAP item Table 3 defines,
typed where edition 1.0 fixes a meaning and carried raw and named where its own §4.6
calls an item "implementation dependent" (`I205/100`, `/120`, `/170`) -- the same
treatment §1.8 already gives items Category 048 does not interpret. `AsterixCat205Codec`
maps a **Sensor Data Report** or a **System Bearing Report** (message types 5 and 2, the
two that carry a bearing) to `Measurement::Bearing`.

**The one design question, and it is the same one this section always pointed at.** No
message type in this category states an *angular* error for a bearing: `I205/110`
Estimated Uncertainty is a positional radius and Table 2 marks it never-present for the
message type that pairs a bearing with a position, and `I205/100` carries no fixed unit at
all. So the codec cannot honestly read a variance off the wire and does not invent one:
`DfSite::azimuth_sigma_rad` is the deployment's own stated accuracy from that direction
finder's Interface Control Document, supplied by the caller the way `RadarSite::
origin_enu_m` already is, and never defaulted -- the same refusal
`gungnir_ingest::adapters::sapient`'s `range_bearing` makes for an unstated azimuth error,
and the same rule the gateway's own validation enforces regardless. A site with no stated
accuracy is simply not configured, the same as an unbound radar. `I205/200` Signal
Elevation decodes but never reaches the measurement for the identical reason: no companion
error exists anywhere in this category, so it is dropped with the loss recorded rather
than given an invented one.

**Deliberately not mapped**: message types 1 and 3, the RDF processing system's own
already-resolved position in WGS-84 or an unnamed Cartesian frame. Converting either to
this deployment's local ENU needs `gungnir-geo`, which this crate may not depend on
(`ARCHITECTURE.md` §7), and trusting an unnamed Cartesian frame to already be this
deployment's would be exactly the unstated-convention error DN-27 §4 warns against. Both
decode losslessly and map to a named `Mapped::NotADetection`.

`gungnir_ingest::adapters::asterix::AsterixFeedAdapter` gained a third category arm and an
opt-in `with_df_sites` builder (`DfBinding` mirroring `RadarBinding`), so no existing call
site changed; `ConfigBaseline`/`gungnir-app`/`gungnir-node` host configuration wiring is
deliberately deferred, the same shape §1.8's own table records for Category 034's host
wiring at the time it landed. No real Category 205 capture exists anywhere to vendor
(checked: EUROCONTROL publishes none, `CroatiaControlLtd/asterix` carries only a
field-definition XML for this category and no sample data, `asterix-specs` carries only
the specification), so `testdata/asterix/cat205.raw` is hand-built directly from edition
1.0's own byte tables and documented as exactly that, never as a real-world recording, in
`testdata/asterix/SOURCE.md`'s Category 205 section.

Tests: six unit tests in the codec (the hand-built record at the specification's own
least significant bits, the no-blocking rule, a reserved FRN, truncation at every length),
four fixture tests in `gungnir-interop/tests/asterix_fixtures.rs` against the hand-built
capture, the catalogue's own conformance and wire-coverage declarations in
`gungnir-interop/tests/conformance.rs`, and two adapter-routing tests in
`gungnir-ingest/src/adapters/asterix.rs`.

## 6. Consequences for GAP-064, GAP-010 and GAP-091

| Half | State on 2026-09-06 | What unblocks it |
|---|---|---|
| ASTERIX Category 048 | **Decoder built 2026-09-06** to edition 1.32; **the radar adapter feeds it** through the gateway (section 1.8) | Host wiring: a radar-feed section in the configuration (socket, bindings) and the host registering the adapter, both untouched here because another change was in those files |
| ASTERIX Category 034 | **Decoder and boundary built 2026-09-06** to edition 1.29; the adapter queues its reports (section 1.8) | A consumer: `gungnir-sensor-management` reading the queue for status and rotation, and the gateway or host carrying it there, which is a gateway change and human-owned |
| STANAG 4676 | Specification not obtained; releasability unverified | Someone with NSO access retrieves AEDP-12 Ed A v1 and records its marking here |
| AIS (GAP-010) | **M.1371-6 pinned, the gpsd captures copied, the decoder built and gated 2026-09-06** (§3.1, §3.3, §3.5; D-32) | A receiver adapter in `gungnir-ingest` and the evidence path into `gungnir-identification` |
| ADS-B (GAP-010) | **Codec built and gated 2026-09-06** on the open-source-consensus route, with **two permissively licensed captures vendored** and **no normative source pinned** (§4.3, §4.5). Doc 9871 2nd edition with Amendment 2 remains the specification of record and is not held | A receiver adapter in `gungnir-ingest` and the evidence path into `gungnir-identification`, the same two the AIS half waits on. Buying Doc 9871 is a later upgrade that changes the oracle column and no test |
| CoT, the schema (GAP-091) | **Version 2.0 pinned 2026-09-06 under D-33**, transcribed in §5.2, publicly released and with the release statement read; no codec, no fixture | A self-recorded corpus (§5.6), then the codec in `gungnir-interop`, the feed in `gungnir-ingest` and the multicast sink in `gungnir-remote` |
| CoT, the type tree (GAP-091) | **Pinned 2026-09-07** (§5.7): the August 2005 MITRE Developer's Guide, case #06-0249, for the grammar and semantics, and the `friend` predicate `^a-f-` for the one question DN-25 asks of a type. The predicate file itself is unobtainable and the mapping file (Apache-2.0, revision 1.80) is named rather than copied | The corpus (§5.6 item 2), which is what turns the 2005 citation into a check against a client |
| CoT, the protobuf framing (GAP-091) | **Pinned 2026-09-08** (§5.4, D-33(e)): `TAK-Product-Center/atak-civ` tag 5.5.1.8, the framing and the ten-message set transcribed in §5.4.1 and §5.4.2; the codec learns its field numbers from that transcription and from nothing copied (D-33(f)) | The corpus's mesh half (§5.6), which a stock client sends as protobuf, then the codec's protobuf half gated on it |

When either codec is built, replace the corresponding `NotImplemented` and cite this note
and the pinned edition from the codec's doc comment, so the citation check in
`docs/README.md` keeps them together.

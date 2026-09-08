# MISB ST 0601 KLV fixture

Copied 2026-09-08 from `github.com/paretech/klvdata`'s `data/` directory, at master
commit `79028b4ab4ce7192d1b7c04d2266fc31ac337511` (2025-01-07), under GAP-099 and on the
terms `docs/design/external-standards.md` §1.5 sets for the ASTERIX capture and §3.3
follows for the AIS logs:

- Test data only. Never linked into, embedded in, or shipped with a binary.
- Provenance recorded here: the repository, the commit, the file's blob id in that
  commit, and the SHA-256 of the copy.
- Decoding it is a conformance check against a real (if third-party, MIT-licensed)
  worked example, not a substitute for MISB ST 0601's own text -- which this project
  could not obtain; see `docs/design/external-standards.md` §8 and §8.2 for why
  (both NGA registry pages that carry it sit behind a bot gateway) and for exactly
  what was cross-checked against this secondary source instead.

**Why this file and not the alternatives `docs/design/external-standards.md` §8.1
surveyed.** `samples.ffmpeg.org`'s KLV samples state no licence anywhere and must not be
vendored per this project's own rule. `github.com/WestRidgeSystems/jmisb` (also MIT) was
the other redistributable candidate; its examples generate synthetic video and metadata
rather than shipping a static worked-example packet, which is a worse fit for a decode
fixture that must stay byte-identical across runs. This file is the smallest, most
direct fit: a single static, complete KLV frame.

**Licence.** `paretech/klvdata` is MIT (`KLVDATA-LICENSE.txt`, copied beside this file
from the repository root at the same commit). The file itself carries no licence header
of its own (it is binary data), so the repository's MIT licence is what covers
redistributing it, exactly as gpsd's compilation copyright covers its `.log` files with
no per-file header.

**What is in it.** One complete MISB ST 0601 UAS Datalink Local Set KLV frame, 228 bytes:
a 16-byte Universal Label key, a 2-byte BER long-form length (`0x81 0xD2` = 210), and a
210-byte value holding 24 local-set items. klvdata's own test suite
(`test/test_misb.py::ParserSingleShort::test_st0601_1`) names its origin as "MISB
ST0902.5 Annex C ... 'Dynamic and Constant' MISMMS Packet Data", with the comment "Some
errors may have been hand corrected" -- i.e. this is MISB's own published worked
example, as transcribed by a third party, not a live sensor capture.

**What was verified, and how (2026-09-08).** `klvdata`'s own Python code (the fetched
commit's `klvdata/misb0601.py`, `klvdata/setparser.py`, `klvdata/klvparser.py`,
`klvdata/common.py`) was read directly and also *run* against this exact file's bytes,
decoding every tag it defines a parser for. That run is the oracle
`gungnir-interop/tests/misb0601_fixtures.rs` checks this workspace's own decoder
against; the values are transcribed into that test file rather than re-run from this
repository, since klvdata is not and must not become a workspace dependency (it is
Python, and per `docs/agentic-coding-standards.md` §2.9 no new dependency was added for
this decoder at all).

**The checksum does not validate, and that is a finding, not an error in this
transcription.** Tag 1 (Checksum) states `0xAA43`. The additive checksum
`klvdata.common.packet_checksum` computes over this file's own 226 bytes preceding that
pair -- independently re-derived by hand and confirmed to match `packet_checksum`'s
output before being trusted -- is `0x3E1E`. Every other plausible byte range (excluding
the checksum element entirely, summing the local-set value only) was tried and none
reproduces `0xAA43` either. `paretech/klvdata` issue #7 (the maintainer's own
explanation of `packet_checksum`'s contract, quoting MISB ST 0601.8-08: "All instances
... where the computed checksum is not identical to the included checksum shall be
discarded") was read for confirmation that this function's usage (the whole packet,
its own trailing two bytes excluded) is the intended one, not a misreading on this
project's part. This is consistent with the file's own test-suite comment that "some
errors may have been hand corrected" in transcribing the original worked example.
`gungnir_interop::misb0601::decode_frame` decodes the frame's fields regardless (KLV
framing does not depend on the checksum validating) and reports the mismatch on
`Misb0601Frame::checksum_valid()`; `gungnir-ingest`'s adapter is where MISB's own
"shall be discarded" rule is applied.

| File | klvdata blob | SHA-256 of the copy |
|---|---|---|
| `DynamicConstantMISMMSPacketData.bin` | `92373298514d60ab3608eae04df9f1967bca63e9` | `73c235eb38a1b80e0a7d0f8a65e013099d602d093d72d34561dde4cec0d5a04a` |
| `KLVDATA-LICENSE.txt` | `69b1c017d431c7867b5cfde2c952fbeb8a0e47d0` | (repository licence text, copied verbatim) |

Source: <https://github.com/paretech/klvdata/tree/79028b4ab4ce7192d1b7c04d2266fc31ac337511/data>.

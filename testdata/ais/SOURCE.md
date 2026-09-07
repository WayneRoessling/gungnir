# AIS fixtures

Copied 2026-09-06 from the gpsd project's regression suite, `test/daemon/`, at master
commit `46977faa8f94eb036ca68c24a3fc0d233732e707` (2026-08-20), under D-32 and on the terms `docs/design/external-standards.md`
§1.5 set for the ASTERIX capture:

- Test data only. Never linked into, embedded in, or shipped with a binary.
- Provenance recorded here: the repository, the commit, each file's blob id in that
  commit, and the SHA-256 of the copy.
- Decoding them is a conformance check against real transmitters, not a substitute for
  ITU-R M.1371. A field that decodes from a capture but disagrees with the pinned edition
  is a bug in the decoder, not a reading of the capture.

**Licence.** gpsd is BSD-2-Clause (`GPSD-COPYING.txt`, copied beside these files from the
repository root at the same commit). The log files carry no licence line of their own;
they are covered by the project's compilation copyright as stated in that file.

**What was changed in the copies.** Each `.log` opened with a comment block naming the
person who contributed it and their e-mail address. Those lines were removed from the
copies: a fixture needs the sentences, not the contributor, and a name and address do not
belong in this repository. The `.chk` files are gpsd's own decodes of the sentences, kept
verbatim as the oracle; they carry vessel identifiers (MMSI, call signs, ship names) that
the vessels broadcast in the clear, which is what AIS is.

**What is in them.** `!AIVDM` sentences (received) with gpsd's JSON decode of each in the
`.chk` file: sentence counts `{'ais-nmea.log': 183, 'ais-raw-messages.log': 114, 'ais-18-27.log': 276, 'ais-nmea-type6-fid55.log': 1389, 'ais_unpack_sixbit.log': 20}`.
`ais-nmea.log` mixes GPS sentences with AIS, which is what a real NMEA feed looks like.

| File | gpsd blob | SHA-256 of the copy | Note |
|---|---|---|---|
| `ais-nmea.log` | `dfd6b34e40f95e4bef686bcf1775c2e6785312bc` | `e7a979cf547b137076960b4023268298a59f7014f89722571c913ec0bb64a3e4` | copied without its 4-line contributor header (a name and an address); the original's SHA-256 was `ac2d9f36e45790677bf6dea5b553869309ef29de6ef40f78a873305dc197dd0e` |
| `ais-nmea.log.chk` | `75f881e3a7902fd55f901f9c9da95e3b3d795c5d` | `089ac9edfeb27950638371aff8a00f56f7a5cad32aaed350ed7b92e10dada0e5` |  |
| `ais-raw-messages.log` | `51e197f67173b2f57d846dcd784cebf30f4f6f1b` | `c53b8d94d9086a1b4cf5d077ba56482e79fa55f8a432b13afb01c99d9d4eadee` | copied without its 7-line contributor header (a name and an address); the original's SHA-256 was `1154fb35e3528c4cf6e0735ab3c2c11cf7773b9a983b6a47f94d1fd31ad757f3` |
| `ais-raw-messages.log.chk` | `38b986b5846e3ab41a173f19b566c0d05afdacfb` | `8ffedc9161e3232a5d347d2d860448315fe170085c86ec215a45a741ce2401bd` |  |
| `ais-18-27.log` | `67672650f22d4989a86aebef6b3d0a17624181dd` | `6b05c303577f6f658ee354e6dbee7f8f0d713abc56352013466496a3831d5ec8` | copied without its 4-line contributor header (a name and an address); the original's SHA-256 was `f742df75fcb3e109238731990fb2dc38b3acb0a372d775eef79ef88b9888d3bb` |
| `ais-18-27.log.chk` | `4cbc5b9db5d5ce2ffde2151b71d691092f0598b1` | `ef0bf47776d3827025615cd800cc33c04575af56fc15951b09d299df80ccdeab` |  |
| `ais-nmea-type6-fid55.log` | `e4ac2d3025fb795d79899c65b78944ad7d8c1c7f` | `186829404c56e242f00616b598d763895835dae16d9db582b9eedcadd598be75` | copied without its 7-line contributor header (a name and an address); the original's SHA-256 was `18bce35eae17b768a925907f25c3df3cde67fa93294b0a71dacb9ff985607386` |
| `ais-nmea-type6-fid55.log.chk` | `e8b79980135c5586340fe540947aed8823ae26e0` | `30bc114c333ad9eaf342a346d4206d8c56054bbed8421b1b139491ed959e2b77` |  |
| `ais_unpack_sixbit.log` | `a268cc97e88ce04939efe76d6671d888faaef224` | `1a6e03c385a8416eaedb93a145886e934a84666c160252b05f362682add1955f` | copied without its 7-line contributor header (a name and an address); the original's SHA-256 was `ab7f50cdc08c2847e5e660fb7cb313acc1707753b3d3770e92328277d16b8482` |
| `ais_unpack_sixbit.log.chk` | `b59273f25e2e9b5562ecde8debda44ec88d6af3b` | `03ed7d18bec84348d3aa73da1fa7db02861f041008759df015f8b64cd58c1fc3` |  |

Source: <https://gitlab.com/gpsd/gpsd/-/tree/master/test/daemon>.

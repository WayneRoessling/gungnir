# GAP-245: two feed parsers no longer crash on what the fuzzer found

The fuzz nightly of 2026-10-29 was the first to run GAP-213's new targets, at 083e39fa.
Two of them crashed on their first night. Both crashes are in `gungnir-interop` codecs,
which are gated on their fuzz targets (`docs/agentic-workflow.md`), and both could be
reached from a sensor feed in a release build.

## The MISB ST 0601 frame length

`decode_frame` added a frame's header length to its declared value length without a
check. A BER length can occupy eight bytes, so a frame declaring a length near
`usize::MAX` overflowed the sum:

- in a debug build, a panic (`attempt to add with overflow`, `misb0601/mod.rs:558`);
- in a release build, a wrapped end smaller than the header, and then a panic on the
  slice between them.

The fuzzer's input was the vendored frame followed by bytes that resynchronized onto a
second key with such a length.

**Fixed.** The sum is checked, and a frame whose end is past the last address is
`BerLengthMalformed`, the error for a frame whose declared length contradicts itself. The
adapter already resynchronizes past that error. `decode_local_set` had the same unchecked
`pos + len` for an item inside a frame; it is checked too and returns
`LocalSetTruncated`. That one had not been hit yet.

## The ADS-B AVR body

`parse_avr` checked that a body was an even number of digits by counting its **bytes**,
then paired its **characters**. A body such as `é`, two bytes and one character, passed
the check and made no pair. The octet list was then empty, and reading the downlink
format from its first octet panicked (`adsb/mod.rs:329`). The fuzz target reads its input
as lossy UTF-8, so replacement characters reached it. A body odd in characters but with
an even pair before the odd character silently dropped the last digit.

**Fixed.** The check counts characters, so both shapes are `Malformed`.

## How it was checked

- **The crash inputs are kept** as `gungnir-fuzz/regressions/<target>/`, byte for byte
  (`-text` in `.gitattributes`).
- **`gungnir-ingest/tests/fuzz_regressions.rs` replays them** through the adapter exactly
  as the fuzz targets do: the same construction and two polls. With the fixes set aside,
  both replays failed with the nightly's own panics at the same lines. With the fixes,
  both pass.
- **Three unit tests** pin the behaviour itself:
  - a frame length past the last address is malformed;
  - an item length past it is truncated;
  - four bodies, each even in bytes and odd in characters, are malformed.
- **`gungnir-interop` and `gungnir-ingest`'s tests** all pass, 265 in all, and clippy is
  clean on both.

## What this does not do

- **The other five targets** passed the same night. Nothing here says they cannot crash;
  each night is another search.
- **The replay directory is new** and holds these two inputs only. A crash the nightly
  finds later belongs there too, with the fix.

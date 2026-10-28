# GAP-213: every feed adapter is fuzzed

Plan 12 tranche B, the sixth build. The review's §5 finding 9 made two claims:
- five of the seven ingest parsers had no fuzz target;
- a node baseline accepted `AllowAllAuthenticator`.

The first was nearly right. The second was not.

## What was found

**Two of the five were already fuzzed.** The `asterix_feed` target has bound a direction
finder and a UAS gateway since the Category 205 and 129 seeds were added, so arbitrary
datagrams reach both decoders and their mapping through the adapter that runs them. Four
had nothing: SAPIENT (the largest parser), MISB, ADS-B and AIS.

**No baseline can select `AllowAllAuthenticator`.** Nothing in `gungnir-config` names it,
and neither binary constructs it: the gateway's authenticator is built from the baseline's
sensors and machine identities. Validation would have had nothing to refuse. What could
change that is code, so code is what is now checked.

## What was built

- **Four fuzz targets** in `gungnir-fuzz`: `sapient_feed`, `misb_feed`, `adsb_feed` and
  `ais_feed`. Each builds its adapter over a recorded source holding the fuzz input and
  polls it, as `asterix_feed` does, so the framing, the decoder and the mapping are all
  fuzzed.
- **Seeds cut from `testdata/`.** A SAPIENT spotter session, four 200-line windows of the
  ADS-B capture, and AIS raw and NMEA logs. For MISB, the vendored frame and a copy with its
  checksum corrected. The vendored frame's own checksum does not validate, so on its own it
  would only ever start the fuzzer on the discard path.
- **`gungnir-ingest/tests/fuzz_seeds.rs`** builds each adapter the way its target does and
  requires every seed to poll and each corpus to reach a detection, as
  `asterix_seeds.rs` does for ASTERIX.
- **The nightly runs seven legs**, the four new targets beside the three it had.
- **`no_shipped_source_admits_every_sensor`** in `architecture_compliance.rs` refuses a
  shipped source that names `AllowAllAuthenticator` outside a test module, and proves itself
  on a planted use in the node.
- **`.gitattributes`** keeps the corpora byte-exact, as `testdata/` is.
- **The fuzz crate's own lockfile** was refreshed for the `rand` packages `gungnir-ingest`
  had gained since it was last built, which the nightly would otherwise resolve itself.

## Decisions taken under the owner's delegation of 2026-10-28

- **The gap's second action was replaced by a check on the code.** It said to refuse
  `AllowAll` in a node baseline, and no baseline can name it.
- **No separate Category 205 or 129 target.** `asterix_feed` already reaches both
  decoders through the adapter.

## What this does not do

The targets were compiled here and their seeds decode, but they have not yet run under
libFuzzer. That happens on the next nightly, which needs nightly Rust and a Linux runner.

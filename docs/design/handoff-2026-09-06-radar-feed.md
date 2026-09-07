# Handoff: the ASTERIX radar feed (GAP-064 closed for ASTERIX, GAP-001 radar half)

Written 2026-09-06 for whoever continues this work. Everything below is verifiable from
the repository; nothing depends on the conversation that produced it. Read
[`external-standards.md`](external-standards.md) first: it is the design record, and
sections 1.8 and 3 say what is built and what is open.

## 1. State at handoff

| Piece | State | Where | Proof |
|---|---|---|---|
| Specifications located, editions pinned | Done, decided by the owner | `external-standards.md` §1.6, §1.7 | The editions are in the codec doc comments and constants |
| Test fixtures | Done, decided by the owner (GPL captures as test data only) | `testdata/asterix/`, `SOURCE.md` | SHA-256 per file, commit recorded |
| ASTERIX framing (Part I 3.1) | Built | `gungnir-interop/src/asterix/mod.rs` | 5 unit tests |
| Category 048 decoder (edition 1.32, Appendix A 1.13) | Built, decode only | `gungnir-interop/src/asterix/cat048.rs` | 9 unit tests, 7 fixture tests in `gungnir-interop/tests/asterix_fixtures.rs` |
| Category 034 decoder (edition 1.29) and the `ServiceMessageCodec` boundary | Built | `gungnir-interop/src/asterix/cat034.rs`, `gungnir-interop/src/lib.rs` | 5 unit tests, same fixture file |
| Radar adapter | Built, receive only, not registered by any host | `gungnir-ingest/src/adapters/asterix.rs` | 6 unit tests; `gungnir-ingest/tests/asterix_feed.rs` (through the real gateway) and `asterix_seeds.rs` |
| Fuzz target and corpus | Built, compiling, in the nightly matrix | `gungnir-fuzz/fuzz_targets/asterix_feed.rs`, `gungnir-fuzz/corpus/asterix_feed/`, `.github/workflows/fuzz-nightly.yml` | `cargo check --bin asterix_feed` in `gungnir-fuzz` |
| Dependency edge (i) `gungnir-ingest` to `gungnir-interop` | Drawn and recorded | `ARCHITECTURE.md` §7.1, `dependency-edges.md` §1 and §4a | `cargo check --workspace --all-targets` passes |
| STANAG 4676 | **Not obtained.** Codec returns `NotImplemented` | `external-standards.md` §2 | Blocked on a person with NSO access |

Commands that must stay green:

```bash
cargo test -p gungnir-interop -p gungnir-ingest
```

```bash
cargo clippy -p gungnir-interop -p gungnir-ingest --all-targets
```

Both were clean at handoff. The workspace-wide test run was **not** green at handoff
because of a compile error in `gungnir-reporting/src/measures.rs` that belongs to
another change in flight (measures catalogue, MOE-05); it is not caused by this work
and should not be fixed from here.

**Update, later on 2026-09-06, by the session that owned the change in flight.** Four
things above have moved, and the words above are left as they were written:

- The workspace-wide gate is green: `cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets` clean, `cargo test --workspace` 958 passed, the UI harness
  86 passed, with this work under it. The measures change landed.
- The change in flight in `gungnir-config/src/lib.rs` that §4 step 1 waits on has
  landed (`ConfigBaseline.revision`, a per-promotion counter). Step 1 can proceed.
- The dependency edge is **(j)**, not (i): (i) had been assigned to `gungnir-app` to
  `gungnir-decision` earlier the same day. It is drawn as (j) in `ARCHITECTURE.md` §7.1
  and listed in `dependency-edges.md` §4a; and it was reviewed -- `dependency-edges.md`
  §7, accepted by the owner as engineering reviewer on 2026-09-06, on the evidence of
  `gungnir-app/tests/dependency_graph.rs`, which now checks every edge's direction and
  the graph's acyclicity on every `cargo test`.
- The register had not been updated for this work: GAP-064 still said "deliberately not
  built" beside a built codec, and GAP-001 did not mention the adapter. Both entries were
  reconciled from the code and stay Open (STANAG 4676; every sensor class but radar), and
  `ARCHITECTURE.md` §10 item 81 records the work and points here. §4's steps are the
  entries' actions now.

## 2. Facts the capture established that the design must honour

These came from decoding the public capture and are recorded in
`testdata/asterix/SOURCE.md`. Each one changed the code.

1. **Categories interleave inside datagrams.** 20 of 100 datagrams carry an 048 block
   followed by an 034 block. Demultiplexing is per data block with
   `gungnir_interop::asterix::data_blocks`, never per stream. The 048 decoder refuses
   an 034 block by name.
2. **Short frames are padded.** 12 frames are padded to Ethernet's minimum. The UDP
   length bounds the payload. The adapter receives datagrams, so a socket already gives
   the right length; a capture reader must trim.
3. **It is a multi-radar feed.** Seven radars, SAC 25, SIC 11, 12, 13, 14, 201, 204, 205.
   Bindings are per SAC/SIC, and a report from an unbound pair is counted, never guessed.
4. **Radar clocks run behind the receiver, not ahead.** 0.5 s to 20 s behind. The
   gateway's rule that source time may not lead receipt time by more than 1 s holds for
   the whole capture. A feed whose radar clock runs ahead would be quarantined by that
   rule, which is the rule doing its job.
5. **Some radars are 3D.** 48 of 126 detections carry I048/110 and map without loss.
   The other 78 record a loss on the provenance (pressure altitude from Mode C, or no
   height).
6. **Some service messages carry status.** 10 of 34 carry I034/050, all "released for
   operational use". The rest carry only type, time, and sector.

## 3. The shape of the code, in the order data flows

```
UDP socket or replay          gungnir_ingest::adapters::asterix::DatagramSource
   │  datagram
   ▼
AsterixFeedAdapter::handle_datagram      splits with asterix::data_blocks
   ├─ category 48 ─► cat048::decode_block ─► AsterixCat048Codec::map ─► DetectionView
   │                                          (RadarSite from RadarBinding + LocalFrame)
   ├─ category 34 ─► cat034::decode_block ─► AsterixCat034Codec::map ─► RadarServiceReport
   │                                          queued; host calls drain_service_reports()
   └─ other       ─► counted as unsupported_category_blocks
   ▼
ProtocolAdapter::poll  ─►  IngestGateway::tick  ─►  authenticate, validate, submit
```

Two rules that shaped every layer and must survive any refactor:

- **A service message is not a detection.** It does not go through `DetectionCodec` or
  the gateway's detection path. That is why `ServiceMessageCodec` and the adapter's
  queue exist. Feeding an 034 message in as an empty detection list would be the
  silent stub the hard rules forbid.
- **Unknown is an error, absent is absent.** An unbound radar is `UnknownRadar`. A
  missing I034/050 yields `status: None`, never "released". A record with no height gets
  the radar's height and a recorded loss, not a silent zero.

## 4. What to do next, in order

**Done 2026-09-06 (GAP-001, GAP-064): steps 1 to 3.** `radar_feeds` is the section step 1
asks for; both hosts bind and register per step 2; step 3 took the host-drain route through
an observation sink, with the registry confirming `Search` on a north marker. Steps 4 to 6
remain.

1. **Configuration for a radar feed.** `gungnir-config` has `SensorConfig` (id,
   modality, geodetic position) and `EndpointConfig` (name, kind, address) but nothing
   that says "this sensor is ASTERIX SAC/SIC x/y on this socket". Add a section, for
   example `radar_feeds: Vec<RadarFeedConfig { name, bind_addr, multicast: Option<(group,
   interface)>, radars: Vec<{ sensor_id, sac, sic }> }>`, validated like the other
   sections (an unknown `sensor_id`, a duplicate SAC/SIC, or an unparseable address fails
   validation rather than defaulting). Record the schema delta in
   `docs/design/model-and-schema-deltas.md` the way the notes do. **Do this after the
   change in flight in `gungnir-config/src/lib.rs` has landed**, not beside it.
2. **Register the adapter in both hosts.** `gungnir-node/src/main.rs` builds the gateway
   near line 71 and its `LocalFrame` near line 177; `gungnir-app/src/state.rs` builds
   the gateway near line 348 and gets the frame from `sustainment::local_frame`. For each
   configured feed: `UdpDatagramSource::bind`, `RadarBinding` per radar with the position
   from `SensorConfig`, `AsterixFeedAdapter::new`, `gateway.add_adapter`, and
   `set_expected_adapters` raised by one per feed. A deployment with no local frame gets
   no radar adapter and a log line saying why, the same rule as coverage drawing.
3. **A consumer for the service reports.** The adapter queues `RadarServiceReport`s and
   nothing reads them. The consumer is `gungnir-sensor-management`: north markers and
   rotation period give the antenna period, sector crossings give the scan phase, the
   status gives operational release and overload. Carrying reports from the adapter to
   that crate means either the host drains the queue and feeds the registry directly, or
   the gateway grows a service path. **The gateway is human-owned**
   (`docs/agentic-workflow.md`): draft, do not merge unsupervised. The host-drain route
   needs no gateway change and is the one to try first. Note the trust rule that
   applies either way: a service report changes what the registry *believes* about a
   sensor, and DN-11 §5 rule 1 says the registry may report only confirmed state.
4. **Read the first nightly fuzz report** for `asterix_feed`. The target is in the
   matrix of `.github/workflows/fuzz-nightly.yml` as of this handoff but has never run
   for its twenty minutes; the seed test only proves the corpus decodes, not that
   mutation finds nothing.
5. **Wire the queue into health.** `AsterixFeedStats` has every count a health panel
   would want (malformed, unknown radar, unsupported category). Nothing publishes it. The
   sustainment panel is the natural place; the gateway exposes `stats()` but not the
   adapters', so the host has to keep a handle to the adapter or the adapter has to
   report through a trait the gateway can call. Prefer the second, but it is a gateway
   change.
6. **STANAG 4676.** Nothing can be built. Someone with an NSO account retrieves AEDP-12
   Edition A Version 1 and its XML schema, reads the cover marking, and records it in
   `external-standards.md` §2. Until then `Stanag4676Codec` stays `NotImplemented` and
   DN-18's coalition exchange has one wire format, ASTERIX, which is a detection format,
   not a track format.

## 5. Things that look like bugs and are not

- `AsterixCat048Codec::default()` decodes nothing into detections: it has no bindings,
  so every record is `UnknownRadar`. Use `decode_records` for the lossless layer or
  `new(sites)` for mapping.
- `DetectionCodec::decode` on the 048 codec returns fewer detections than records: `TYP =
  0` and position-less records are dropped from the detection path by design; `map`
  returns `Mapped::NotADetection(reason)` for them.
- The 048 codec's `encode` is `NotImplemented` on purpose. Gungnir is the surveillance
  data processing side; a fused track is not a monoradar report.
- `RadarBinding.position` is geodetic and the codecs want local ENU; the adapter converts
  with the deployment's `LocalFrame`. The codecs cannot, because `gungnir-interop` may
  not depend on `gungnir-geo` or `gungnir-coord` (it depends on the model alone, and the
  model's `LocalFrame` is what the adapter uses).
- `source_time` folds I048/140 onto the receipt date within twelve hours. A capture
  replayed with a `now` far from its own day will place source times on `now`'s day, not
  the capture's; that is why the fixture tests pass the packet timestamps as `now`.
- The adapter's `handle_datagram` is public so the fuzz target and the seed test reach
  the parser without a socket. Not for hosts.

## 6. Where the records live

| Question | Document |
|---|---|
| Which edition, and where the PDF is | `external-standards.md` §1.1 to §1.7 |
| What is built and what each layer refuses to do | `external-standards.md` §1.8 |
| What is still open, by half | `external-standards.md` §5 |
| Where the fixtures came from and what decoding them established | `testdata/asterix/SOURCE.md` |
| The dependency edge and its justification | `ARCHITECTURE.md` §7.1 edge (i); `dependency-edges.md` |
| The pass criteria this work must keep meeting | `docs/verification-capability-table.md`, the `gungnir-interop` and `gungnir-ingest` rows |
| The decisions the owner took, with dates | `docs/design/README.md`, "Decisions taken 2026-09-06" |
| GAP-064 and GAP-001 status in the register | `ARCHITECTURE.md` §10, the GAP-064 paragraph and the "Live protocol adapters" bullet |

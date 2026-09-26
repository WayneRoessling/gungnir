# The interop, model and observability rows get their tests

GAP-116, GAP-117 and GAP-125
([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
each filed by the GAP-067 walk ([`../2026-09-16/gap-067-walk.md`](../2026-09-16/gap-067-walk.md))
against a row it held for want of a test. The tests are built; no row or criterion in
`docs/verification-capability-table.md` changed, and each row waits for the owner's walk.
Two of the three sets of tests found something, and a third finding is filed.

## GAP-116: the seven interop clauses

`gungnir-interop/tests/asterix_fixtures.rs` and `tests/conformance.rs`, one test per
clause, each naming its clause.

1. Every Category 034 record of the capture is mapped; a report carries a status exactly
   when its record carries I034/050's common subfield. Pinned to `SOURCE.md`'s own count
   made when the capture was copied: 10 with a status, all nominal, and 24 with none.
2. A System Bearing Report carrying I205/080 (with I205/060) and one carrying only
   I205/070 (with I205/050) both map to `Measurement::Bearing` with the site's own
   variance. Every earlier Category 205 test was type 5, so the type 2 arm had never run.
3. `check(name, version - 1)` is refused with `IncompatibleSchema` for every catalogue
   entry, beside the `version + 1` that was already checked. "This version or older"
   would have passed the old test.
4. A lossy 034 message carries its loss: a jamming strobe names its polar window, and a
   message with no I034/030 is timed at receipt and says so. Every capture message, all
   timed crossings and north markers, carries none.
5. The capture's Category 048 records carry exactly one uninterpreted item, I048/230, as
   `SOURCE.md` says, two octets each; its 034 records carry none; a hand-built 034 record
   carrying I034/SP carries it under that name, without its length octet.
6. Every block of the capture, and each single-block fixture, is cut at every length and
   **its length field rewritten to match**. The old test cut the datagram, so every cut
   failed the framing's length check and no category parser ever saw a short record: "no
   truncation panics in any of the four decoders" was true of the framing alone. Now each
   cut reaches its own parser, which refuses it at an octet inside the record, or, on a
   record boundary of a blocked category, returns exactly the records before the cut.
7. A System Position Report and its conflicting counterpart decode with their data source
   and time, and map to `NotADetection` with a reason naming the message type and the two
   types that do map.

**What clause 4 found.** The time fold every category shares,
`asterix::cat048::source_time`, worded a missing time of day as "no time of day
(I048/140)" whichever category called it. A Category 034, 205 or 129 report with no time
named an item its record cannot carry, on the field an auditor reads to learn what was
lost. The caller now passes its own wording, and each category module exports it as
`TIME_OF_DAY_ABSENT`, naming I034/030, I205/030, I129/070 or I048/140.

## GAP-117: every event and view type through serde

`gungnir-model/tests/serde_round_trip.rs` round-trips one value per variant of all 22
enums in `gungnir_model::events` (82 variants) and every `*View` type the crate declares,
with non-dyadic floats and finite values, and requires the same value back and the same
text on re-encoding. An older `SCHEMA_VERSION` is refused with both versions named.

Forgetting a new one is made to fail three ways. Every value is a full struct literal, so
a new field stops the file compiling. Every enum has an exhaustive match with no wildcard,
so a new variant stops it compiling, and each arm's name is checked against the tag serde
writes. And the test reads `events.rs` and every module `lib.rs` declares: a new enum, a
new variant or a new `*View` type with no value in the tables fails the test by name.

**What it found: GAP-175.** Taking each value's tag with `serde_json::to_value` failed on
`IdentityEvent` with "number out of range". `GlobalEntityId` is written as a 128-bit JSON
number, which a `serde_json::Value` cannot hold and a reader outside Rust rounds, while
the schema catalogue registers it as RFC 9562 text. D-60 fixed exactly this for the three
record identifiers and did not reach this one. It is filed rather than fixed because
D-60's reader does not help here: through `deserialize_any`, `serde_json` hands a number
wider than 64 bits to the visitor as a float, so switching the writer to a string would
leave every journal written since GAP-069 with identity events it cannot read. Keeping
those readable is a decision the fix needs, and it is GAP-175's action.

## GAP-125: both binaries report health through the monitor (D-100)

Neither binary used `SnapshotHealthMonitor`. Each built `SystemHealth` from its services'
flags and kept its own "what was said last" -- `AppState::health_journaled` beside
`AppState::health` on the desktop, `Announcer::last_health` on the node -- with its own
comparison, and the monitor's one test set a health once.

D-100 routes both through the monitor, which now answers what a report means:
`SnapshotHealthMonitor::report(health, at)` stores it and returns the `HealthEvent` to put
on the record when it differs from the last report or is the first. The desktop's tick
publishes what it returns and reads the health through `AppState::health()`; the
`health` and `health_journaled` fields are gone, so there is one holder. The node's
`Announcer` holds a monitor in place of `last_health`, and the flag reading moved from
`main.rs` into `picture::read_health`, which the loop, the backend-parity test and the new
test all call.

The tests take each service's flag true, false, true with the other two healthy:
`gungnir-app/tests/health_follows_flags.rs` through `update::tick`, and
`gungnir-node/tests/health_follows_flags.rs` through the node loop's own functions in
`main.rs`'s order, checking the snapshot a connecting desktop reads as well. After every
tick the health is exactly what the services report, a change puts exactly one transition
on the bus, and a repeat puts none. Ingest's flag is the real gateway's: the test's adapter
fails its poll, which is what makes a gateway unhealthy for that tick.

Rejected: testing each binary's own copy of the logic in place, which would have kept two
definitions of a health transition, one per binary, for two tests to hold in step; and
moving the flag reading into `gungnir-observability`, which would need edges from it to
the tracking, intercept and ingest crates that `ARCHITECTURE.md` §7.1 does not draw.

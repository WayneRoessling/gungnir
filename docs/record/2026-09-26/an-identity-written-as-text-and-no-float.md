# An identity written as text, and no float blanked on any wire

GAP-175 and GAP-171 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-101, D-102 and D-103 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)).
The owner took all three on 2026-09-26, answering directly; the parts each left open are
taken under the owner's delegation of the same day and marked so in each decision. Both
gaps were found by earlier work: GAP-175 by GAP-117's round trip
([`the-interop-model-and-observability-rows-get-their.md`](the-interop-model-and-observability-rows-get-their.md)),
GAP-171 by GAP-153 ([`the-report-and-every-float-reach-the-partner.md`](the-report-and-every-float-reach-the-partner.md)).
Found on the way: GAP-176.

## GAP-175: a global entity identity crosses as text

### What was wrong

`GlobalEntityId` holds a UUID v7 as a `u128`, and its derived `Serialize` wrote that as a
JSON number up to 39 digits long. `serde_json::to_value` refused any value holding one
("number out of range"). A JSON reader outside Rust reads such a number as a double, which
keeps 53 bits, so two identities minted in the same millisecond can come out equal and
one identity can come out as two. The schema catalogue has registered the identity as RFC
9562 text since GAP-069, and no document carried that form. D-60 fixed the same defect for
the three record identifiers and did not reach this type.

### The decision (D-101)

The owner decided: **written as the hyphenated RFC 9562 string from now on, and both forms
read.** The old number is read exactly from its raw token, and **not** by turning on
`serde_json`'s `arbitrary_precision`. Cargo unifies features across the workspace, so that
feature would change how every crate's numbers are buffered; D-60 already refused it for
breaking field-tagged enums. Existing journals keep reading, and nothing is rewritten.

The writer is D-60's own: `GlobalEntityId`'s `Serialize` and `Deserialize` call
`gungnir_model::identifier::wire`, as `DecisionId`, `PlanId` and `PendingApprovalId` do. The
declaration stays `pub struct GlobalEntityId(pub u128);`, the shape the UAF generator reads.

### Where the raw token is read, and why not in the type

The obvious place is a custom `Deserialize` on the identity, and **it cannot work**. A
`Deserialize` has to choose one `deserialize_*` call before it sees the token:

- `deserialize_any` is what accepts both a string and a number, and `serde_json` hands it
  an integer wider than 64 bits as an `f64`, whose low bits are already gone;
- `deserialize_u128` reads the digits exactly and refuses a string.

The old derive asked for a `u128`, which is why every old line read until now. A type that
also accepts text has lost that exactness, and `serde_json`'s reader type is sealed, so no
adapter can peek at the token either.

The raw token is therefore read one step earlier, where the text still exists.
`gungnir_eventing::nonfinite::from_line` reads every journal line, and every v3 frame and
body the desktop's link reads. It now passes the line through
`gungnir_eventing::wide_integers::identities_as_text` first. That function scans the JSON
once, outside strings, and writes each non-negative integer token wider than 64 bits and at
most 128 as the hyphenated text of the value its digits spell. A line with no such token is
returned borrowed and untouched, so the cost on an ordinary line is one scan.

**Why any such token is an identity.** Nothing else in the workspace writes an integer that
wide. Every `f64` is written with a point or an exponent, every other integer field is 64
bits or fewer, and the record identifiers have been text since D-60 (whose reader also takes
the hyphenated form). Negative numbers, fractions and exponents are never touched. Outside
`from_line`, a plain `serde_json` read of an old number reaches the identity as a float and
is refused with an error naming the identifier, never rounded.

### The version (under delegation)

`SCHEMA_VERSION` goes from 4 to 5 and the path stays `/v3`. The interface's rule is that a
written field whose type changes needs a new schema version, and a new path only where an
old client could silently misread the payload. A version-4 client cannot misread text as a
number. Without the bump, though, it would meet its first identity frame on the stream,
fail to decode it, and take that for the node ending the stream: the loop GAP-153 closed
for NaN. With the bump, the exact-match check refuses it at the snapshot, before it
connects, with both versions named. A journal written at version 4 still reads.

### How it is tested

`testdata/journals/pre-gap-175/` holds two desktop sessions written by the code before the
change: one object minted, joined again across a restart, and a second object minted. Each
identity is a real v7 that no double holds. Its `SOURCE.md` says how it was written.
`gungnir-app/tests/pre_gap_175_journal.rs` checks four things:

- the fixture is what it claims: each number is wider than 64 bits and is changed by a
  double;
- the journal reads to the exact identities, taken from the raw digits and not from any
  reader under test, and each line re-encodes with the identity as text and reads back
  unchanged;
- each identity event now goes through `serde_json::to_value` and back;
- a desktop restarted over those files restores exactly the two entities.

With `identities_as_text` taken out of `from_line`, the second and fourth tests fail.
`gungnir-model/src/identity.rs` tests the written form, the `Value` round trip, and that a
wide number read directly is refused. `gungnir-eventing/src/wide_integers.rs` tests that
nothing else is touched: `u64::MAX`, floats of any length, negatives, strings, escapes, the
marked form, and a number wider than 128 bits. `gungnir-model/tests/serde_round_trip.rs`
now takes every enum's tag with `serde_json::to_value`, so every value in it is shown to go
into a `Value`, the identity events included.

The catalogue entry, `docs/gungnir-api-v1.md` (a "Schema version 5" section),
`design/model-and-schema-deltas.md`, and the UAF identity rows (`Sd-Tx`, `IE-11`) say so.

## GAP-171: every body a desktop reads is lossless, and a partner is told what is unavailable

### What was wrong

GAP-153 gave the stream, the history and the snapshot D-96's lossless form. The other
bodies were still plain `serde_json`, which writes a NaN or an infinity as `null`:

- The queue a desktop reads with `GET /v3/queue` then failed to decode, and PN-06 had no
  picture of the node's queue at all.
- A decision's `409` names the record that stands, plan floats and all.
- Worse, an exchange product's `body` is a `serde_json::Value`, which cannot hold a
  non-finite float, and `serde_json::to_value` writes one as `null` **without an error**. A
  mission report whose tracker diverged would reach a partner with its MOTA blank, which
  the partner cannot tell from a field the report does not have.

### The decisions

**D-102, the owner's:** our own v3 bodies (the queue, coverage, and any other response body
the desktop reads) take D-96's lossless form, sent as before when every float is finite.
Under delegation, that set is:

- `GET /v3/queue` and `GET /v3/coverage`;
- both session routes;
- the answers to a sensor task, to a decision (`201` and `409`) and to a forwarded batch
  (`202` and `409`).

Each is proved to read back before it is sent, as the snapshot is, and the link reads each
one by its marker. `GET /v3/health` is unchanged: it has no float, and partners read it.

**D-103, the owner's:** a partner-bound product carries an explicit "value unavailable"
marker in place of a non-finite value, a defined JSON shape written into DN-18. **The shape,
taken under delegation, is an object in the number's place:**

```json
{ "unavailable": "nan" }   { "unavailable": "+inf" }   { "unavailable": "-inf" }
```

It was chosen because it is what a partner reading DN-18's format is least surprised by:

- It is plain JSON, which every reader parses.
- It sits where the number was, so it works for a float inside an array or a map, which a
  sibling field cannot reach.
- It says the value is unavailable, which is what the partner needs, and which way. It does
  not carry the sender's bits.
- `null` keeps the one meaning it had in these bodies, an absent optional value.

Four alternatives were rejected:

- the string `"NaN"`, which a partner cannot tell from a string field;
- MongoDB's extended-JSON `{"$numberDouble": "NaN"}`, which describes the sender's
  representation and ties the format to one vendor;
- D-96's lossless line, which is not JSON, so a partner's reader would refuse the whole
  product for one figure;
- refusing the product at publish, which loses every other figure when the unavailable one
  is often the finding.

DN-18 §15 is the partner's contract.

**One implementation.** `gungnir_eventing::nonfinite` gains a third mode beside detect and
escape, wrapping `serde_json`'s own serializer the way the lossless form does, so every
float in every type is covered by construction.

- `to_partner_value` builds a body.
- `to_partner_string` writes the whole exchange answer, so `at` and `as_of` are covered
  too.
- The node's own handoffs call it through `gungnir_api::v3::ExchangeProduct::body_of`.
- The desktop's handoffs, launch warnings and mission report call it through
  `gungnir_app::exchange::product_body`, because a desktop has no production edge to
  `gungnir-api`.

A body that cannot be written at all is now logged by name rather than blanked silently.

### How it is tested

- `a_nan_and_an_infinity_in_a_queue_item_reach_a_linked_desktop`
  (`gungnir-remote/tests/transport.rs`): a real node and a real link. A queue item carries
  a NaN with a payload, an `f32` NaN and both infinities, and the link takes its picture
  through `GET /v3/queue` with every bit intact. The raw body is marked, and an all-finite
  queue is byte for byte `serde_json`.
- `a_nan_and_an_infinity_in_coverage_cross_the_route`: the same for a coverage gap, inside
  the internally tagged `CoverageResponse`.
- `a_report_carrying_a_nan_reaches_its_partner_marked_unavailable_and_a_finite_one_unchanged`
  (`gungnir-app/tests/cut_off_and_reconnected.rs`): a console builds its report product
  with `sustainment::exchange_record`, and the partner reads `GET /v3/exchange/reports` over
  mutual TLS under its own certificate.
  - An all-finite report arrives exactly as `serde_json` writes it, with its body equal to
    `serde_json::to_value`.
  - A report whose metrics are NaN, `+inf` and `-inf` arrives with the three objects, and a
    `None` beside them is still `null`.
  - With `serde_json::to_value` put back, it fails, showing the three `null`s.
- `gungnir-eventing/src/nonfinite.rs` holds the mode itself: identical to `serde_json` when
  finite, and the object for NaN (any sign and payload) and both infinities, in `f64` and
  `f32`, in fields, `Option`s, maps and a tagged enum.

### What is human-owned

Human-owned; see [`../../signatures.md`](../../signatures.md). The `gungnir-api` handlers
changed are the exchange read routes and the answers of four write routes: session,
sensor task, decision and forwarded batch. Only the body's encoding changed; no check,
authority or state change moved. The exchange publish handler is unchanged: a body arrives
as JSON, which cannot carry a non-finite float. `gungnir-api` also gained
`ExchangeProduct::body_of`, used by the node.

### Found on the way: GAP-176

Two other posts build a `Value` the same way: the warning a desktop posts to a warned
party's endpoint, and the handoff posted to an effector. Both still blank a non-finite
figure. Each payload's format belongs to its own contract (DN-03, DN-07), which the owner's
GAP-171 decision did not name, so it is filed rather than widened into this change.

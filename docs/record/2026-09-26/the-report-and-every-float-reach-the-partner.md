# The report and every float reach the partner

GAP-150 and GAP-153 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-96 and D-97 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
taken under the owner's delegation of 2026-09-26. Both were found closing earlier gaps:
GAP-150 by GAP-146 ([`../2026-09-25/a-console-that-may-not-publish-says-so.md`](../2026-09-25/a-console-that-may-not-publish-says-so.md)),
GAP-153 by GAP-126 ([`../2026-09-25/every-float-kept-old-sessions-purged-on-purpose.md`](../2026-09-25/every-float-kept-old-sessions-purged-on-purpose.md)).
Found on the way: GAP-171.

## GAP-150: the report a partner was sent is mission state

### What was wrong

A console publishes its handoffs, its launch warnings and its mission report to coalition
exchange. Since GAP-145 and GAP-146 the link's reconnection edge publishes the first two
again, because that edge is the one moment a console knows its node may have forgotten
them: a node that restarted holds an empty register, and a sign-in on a linked console
builds a new link and drops whatever the old one held. The report was left out. It lived in
PN-13's `ReportState`, inside the window state `SustainmentState`, which the tick does not
reach. A partner reading `GET /v3/exchange/reports` was served nothing from that console
until somebody pressed Generate again, while the report sat on the console's own screen.

### The decision (D-97)

**The report partners were sent is mission state; the panel's drawing of it is not.**
`AppState::exchange_report` holds the record exchange carries: the report's identifier,
its marking, its body, and **the time it was generated**. PN-13's Generate writes it and
queues it; Export queues it again; the reconnection edge (`exchange::republish_all`)
queues it with the handoffs and warnings. All three send the same record, so `at` stays
the report's age. Stamping the time of each resend would tell a partner a stale report was
fresh, which is worse than not sending it.

Two alternatives were rejected:

- **Say on PN-13 that a report is published when it is generated and not again.** That is
  true and it leaves the defect in place. A node restart is not something an operator sees,
  so the sentence would ask a person to regenerate a report whenever something they cannot
  observe has happened.
- **Move the whole `ReportState` into `AppState`.** The counts, the measures lines, the
  export path and the order-of-battle products are what a window draws. The mission depends
  on what partners were told, which is one record.

**Not recovered from the journal at start.** A console that restarts shows no report on
PN-13. Republishing one it no longer shows would put a product in front of partners that
nobody at that console can see or answer for. After a restart the report goes out again
when it is next generated, which is the behaviour GAP-065 built. A console that has
generated no report publishes none. An empty set would claim "there is none here", and
that console does not know it.

GAP-146's outbox rules are unchanged: one set per item, the newest, replacements counted;
a `401` or `403` holds the set until the link signs in again. `ReportState::generate` now
takes the state mutably. That is the honest signature for an action that changes mission
state.

### How it is tested

`a_console_s_mission_report_reaches_its_partner_again_when_its_link_comes_back`
(`gungnir-app/tests/cut_off_and_reconnected.rs`) runs one console against a real node that
has an agreement with a partner, `sector-north`. The partner reads through the real route
over mutual TLS, under its own certificate. The client configuration is the product's
(`gungnir_remote::client_config`) and the node serves through `acceptor_with_key` and
`serve_on_listener`, as the binary does. The test runs as follows:

1. An Operator's console generates the report at T+120. The node refuses the publish (403)
   and the partner is served nothing.
2. PN-13's state is dropped.
3. A Supervisor signs in at T+150. The new link publishes the report, and the partner is
   served it stamped T+120.
4. The node goes down and a new node with an empty register comes up where it was. At
   T+200 the reconnection publishes the report to the new node, and its partner is served
   it stamped T+120.

With the edge's report line removed, the test fails at step 3.
`the_reconnection_edge_publishes_the_last_report_as_it_was_generated`
(`gungnir-app/tests/sustainment.rs`) holds the stamp and the empty case against a scripted
link.

To give the test a real restart, `Proxy::retarget` sends the desktop to a second node at
the same address as far as the desktop can tell. The test needed `tokio-rustls` as a
`gungnir-app` dev-dependency to carry the partner's request. That crate is already in the
graph through `gungnir-remote` and `gungnir-api`.

## GAP-153: a NaN on the wire is a NaN, not the end of the stream

### What was wrong

D-77 made the journal keep an envelope that carries a NaN or an infinity, bit for bit, in
a marked line (`~` and escaped JSON). The node then sent that same envelope to its desktops
as plain `serde_json`, which writes a non-finite float as `null`. The desktop's decoder
failed on the frame and took it for the node ending the stream. It reconnected from the
envelope before that frame, and the node's backlog sent the same frame again, every time,
so the desktop never got past it. A history holding such an envelope failed the
reconciliation's read. A snapshot carrying a diverged track stopped the link from
connecting at all, because it cannot connect without the snapshot. The gap named the stream
and the history; the snapshot is the same defect on the route every connection starts with.

### The decision (D-96)

**The wire carries it the way the journal does.** The codec moved from `gungnir-store` to
`gungnir-eventing` (`src/nonfinite.rs`). The journal, the node's transport and the
desktop's link all depend on `gungnir-eventing`, and neither of the other two may depend on
the store. `gungnir_store::nonfinite` re-exports it, so the journal's callers did not
change. No crate edge was added. `gungnir-eventing` gained `serde_json`, which is already in
the workspace and in both binaries' graphs.

On the wire:

- **A frame or body whose floats are all finite is byte-for-byte what it was**, labelled
  `application/json`.
- **One that carries a non-finite float is the marked form**, labelled
  `application/vnd.gungnir.lossless-json`, because it is not JSON.
- **The desktop reads by the marker, not the header**, the same way the journal reads its
  lines.

An envelope is encoded once, when `NodeApi::publish_event` takes it, and proved to read
back (`nonfinite::to_faithful_line`). Every subscriber is sent that one line, which also
removes a per-subscriber encode. An envelope with no faithful line is refused with
`ApiError::Unencodable` and counted in `NodeApi::unencodable_envelopes`. No such envelope is
known: the node binary journals every envelope to the same test before it offers it. The
refusal is the transport's own guarantee that no stream carries a frame its desktop cannot
read. Without it, one bad frame in the backlog would end every reconnection that replayed
it.

The alternative the gap offered was to refuse and count every non-finite envelope at the
node's publish. It was rejected for D-77's reason: the envelope is a true record, often of
the very fault someone will investigate. With the journal keeping it and the wire refusing
it, the node's history and its journal would disagree. A desktop reconciling after an
outage would then see a divergence that is not one, with no event on its own record to
explain it. Schema and path versions do not move. Nothing a client could read before
changes meaning, and a client that predates this meets a marked frame exactly where it met
the `null`.

### How it is tested

`a_nan_and_an_infinity_cross_the_snapshot_the_stream_and_the_history`
(`gungnir-remote/tests/transport.rs`) runs a real node and a real link. It sends a NaN with
a payload (`0x7ff8_0000_0000_beef`) and both infinities in a diverged track through all
three routes and checks the bits of each. The checks are:

- the snapshot's diverged track arrives;
- the stream's track arrives and a finite envelope behind it arrives on the same
  connection, with no error on the link;
- the history reads back through `fetch_history`;
- the raw history body is marked, and a finite range is plain JSON.

`a_faithful_line_is_proved_to_read_back_and_an_unfaithful_one_is_refused`
(`gungnir-eventing/src/nonfinite.rs`) tests the refusal against a type whose `Deserialize`
does not undo its `Serialize`, and a map `serde_json` cannot key.

### Found on the way: GAP-171

Every other JSON body is still plain `serde_json`: the queue view, coverage, and
above all each exchange product's `body`. That body is a `serde_json::Value`, which cannot
hold a non-finite float at all. `serde_json::to_value` turns one into `null` without an
error, so a mission report with a NaN measure would reach a partner with that figure
silently blanked. Filed as GAP-171 rather than widened into this change: an exchange body
is a `gungnir-api` write path, and the partner's format is DN-18's to decide.

Nothing human-owned was changed. The stream, the history and the snapshot are read paths,
and `gungnir-remote`'s TLS identity path is called from a test and not changed.

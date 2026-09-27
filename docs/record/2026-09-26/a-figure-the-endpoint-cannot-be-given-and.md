# A figure the endpoint cannot be given, and the node's record on PN-20

GAP-176 and GAP-179 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-115 and D-116 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
[`../../design/DN-03-warning.md`](../../design/DN-03-warning.md) §12,
[`../../design/DN-07-handoff.md`](../../design/DN-07-handoff.md) §9 and
[`../../design/DN-23-operator-authentication.md`](../../design/DN-23-operator-authentication.md)
§15. Built on 2026-09-26. Both halves touch human-owned code: the handoff delivery in
`gungnir-approval` (the decision path, D-65), and `gungnir-security`'s authorization with
the `gungnir-api` route it guards. What the owner has signed is in
[`../../signatures.md`](../../signatures.md).

## GAP-176: a warning and a handoff post a NaN as `null`

GAP-171 stopped an exchange product's body blanking a NaN or an infinity (D-103). The two
other posts to an outside endpoint were built the same way and left out of its scope: the
warning a desktop posts to a warned party, and the handoff a desktop or a node posts to an
effector. Each was `serde_json::to_value(...).unwrap_or(Value::Null)`, so a non-finite
figure reached the receiver as `null`, and a value `to_value` refused for any reason sent
the receiver a body of `null` while the record said it was posted.

### What was decided (D-115, under the delegation)

**The same marker the owner chose for a partner.** In the number's place,
`{"unavailable": "nan" | "+inf" | "-inf"}`, built by
`gungnir_eventing::nonfinite::to_partner_value`, the one implementation exchange uses. A
body whose figures are all finite is byte for byte what it was.

- **Never withhold a warning for a figure in it.** A warning whose due time is marked
  unavailable still warns; one that does not go out leaves the party unwarned, and DN-03
  exists because a warning function that fails quietly is worse than none.
- **The same for a handoff.** The effector needs the decision; a quality figure it is told
  is unavailable is something to judge by.
- **What `to_value` refused for another reason is now said.** Neither record has a map
  with non-string keys, so it cannot happen today, but if it did the warning fails loudly
  ("warn by voice") and the handoff is recorded undelivered with the reason, rather than a
  body of `null` counted as posted.

Rejected, for the reasons D-103 gives for exchange: `null` as before (indistinguishable
from an absent optional, which is what `null` already means in both bodies), the string
`"NaN"`, an extended-JSON number, the lossless line of D-96 (not JSON any receiver reads),
and refusing the record at the post.

### What was built

- `gungnir_app::warnings::endpoint_payload` and `gungnir_approval::handoffs::endpoint_payload`,
  each a thin call to `to_partner_value`, used where `to_value` was. A retried handoff
  re-posts the payload it was first given, so a retry is the same bytes.
- `gungnir_app::warnings::post`, the tick's own delivery for one warning, public so a test
  can put a warning carrying a NaN on a real socket: the rule raises one only with a
  finite due time.

Tests, in `gungnir-app/tests/endpoint_delivery.rs` against the stub endpoint on a real
socket: `a_nan_and_both_infinities_reach_the_endpoint_marked_unavailable_in_a_handoff_and_a_warning`
(a handoff built by the decision path from two tracks whose quality is NaN, `+inf` and
`-inf`; a warning whose due time, state and history carry the three) and
`a_handoff_and_a_warning_whose_figures_are_all_finite_are_posted_byte_for_byte_as_before`
(the bytes the endpoint received equal `serde_json::to_vec(&serde_json::to_value(..))`).

## GAP-179: a node's audit record reaches only the node's log

GAP-163 had the node verify its audit record against the heads its journal holds at every
start, and say what it found in its log, in its audit log and on its health line. No route
served the record or that verification, so the security staff at a linked desktop saw the
desktop's record only, and a cut segment on the node was told to a log file a watch floor
may never read.

### What the owner decided, 2026-09-26 (D-116)

A new action, **`audit.read`**, held by the **administrator and the commander**; its §4 row
first; then the action, `role_permits`, and a v3 read route under the lossless body
encoding serving the node's segments, entries paged, and its verification, authorised by
`audit.read`, **with every read itself audited**. A linked desktop's PN-20 shows the node's
record and verification beside its own, read only, and says so honestly when the role lacks
`audit.read` or the node is unreachable.

The security officer, whose layout is health and the desktop's own audit panel (D-30), was
not given it: its one action stays escrow recovery, as the owner decided.

### What was taken under the delegation, and why

- **The route.** `GET /v3/audit`, with an optional query: `verify`, `segment`, `from` and
  `limit` (at most 500, `AUDIT_PAGE_LIMIT`). No query answers the last verification and
  the segment list; a segment answers a page of it as well.
- **The loop answers it, as it answers a decision.** The handler checks the session and
  `audit.read` and hands the read to the node loop through a carrier
  (`PendingAuditRead`), because the audit log and the verification are the loop's and a
  handler never touches the disk. The loop records one `audit.read` entry **before**
  reading the page, syncs it, then answers: a page of the running segment ends with the
  read that fetched it. A refusal is one entry too, under `audit.read` for a missing
  permission or a query that does not read, and `access.refused` for a caller with no
  session, as on every route.
- **A verification on request, against heads held in memory.** The node verifies at
  start. A reader can ask for a verification now ("Verify the node's record now"). Folding
  the journal again would read the whole running session on the loop that tracks, which on
  a node up for days is most of its disk, and a reader could stall the system of record by
  asking. So `NodeAuditRecord` subscribes to the node's own bus before the start's
  verification and applies every `Anchored`, `Verified` and `Purged` it carries to the
  ledger that verification left: the statements the journal appends, in order. A
  verification on request is then `verify_audit_record` against that ledger, which reads
  the audit directory and nothing else. If the start could not read the journal's heads,
  no ledger is held and a request folds the journal the start's way. A verification on
  request says what it found as the start does: journaled, logged, and an
  `audit.anchor_mismatch` entry when anything is wrong, so a read with `verify` on a
  damaged record writes two entries, the finding and the read.
- **Read when a person asks, never polled.** Every read is an entry on the node's record,
  so a desktop that polled would fill it with reads nobody made. PN-20 reads it on "Read
  the node's record", "Verify the node's record now", and a segment's "Show", "Older" and
  "Newer".
- **Said, not left blank.** A desktop with no node draws no section. A role without
  `audit.read` is told so, naming the roles that hold it, and nothing is sent: a read the
  node would refuse is still an entry on its record. A refusal the node gives anyway is
  drawn with its reason. A node that could not be reached is said to be, and what an
  earlier read showed stays, labelled with when it was read. A damaged record raises an
  alert on the desktop as its own damage does.

Rejected: carrying the node's last verification on its health line to the desktop (the
gap's first suggestion). Health is served to every role and to partners; the verification
names files and counts on the audit record, which the owner put behind `audit.read`.

### What was built

- `docs/mission/roles-and-stakeholders.md` §4's new row, and its transcription in
  `gungnir-security/tests/role_matrix.rs`; `gungnir_security::actions::READ_AUDIT`, in
  `ALL`, and granted to the commander in `role_permits` (the administrator holds it by its
  arm).
- `gungnir_api::routes::AUDIT`, the v3 types `AuditQuery`, `AuditRecordResponse`,
  `AuditVerificationView`, `AuditSegmentView`, `AuditPageView` and `AuditEntryView`, and
  the handler in `gungnir-api/src/transport.rs`.
- `gungnir_node::audit_record::NodeAuditRecord`, wired in `gungnir-node/src/main.rs`
  before `audit_routes` each tick.
- `gungnir_remote::link::fetch_audit_record`, `gungnir-app/src/node_audit.rs`, and PN-20's
  section in `gungnir-ui/src/panels/audit.rs`.

Tests: `gungnir-app/tests/node_audit_on_pn20.rs` (a node whose earlier segment was cut,
linked to a desktop over mutual TLS: the administrator and the commander read the record
and the cut shows on PN-20, a page of the cut segment and a verification on request
included, each read exactly one entry naming the operator and the desktop's machine; an
Operator's console says it may not and sends nothing, and the route refuses the Operator's
session `403` with one entry; the node gone is said, and the earlier read stays);
`gungnir-node/tests/node_audit_record.rs`
(`a_verification_on_request_finds_a_cut_made_while_the_node_runs`, the in-memory heads
finding a cut made to an earlier segment while the node is up);
`gungnir-security/tests/role_matrix.rs`; and `gungnir-ui/src/panels/rendered.rs` (PN-20
draws the node's section).

**Found building it:** cutting an earlier segment's tail also breaks the next segment's
first line, because the chain runs across segments. A verification reports both, the cut
and the break, and says the same at a start; nothing was changed for it.

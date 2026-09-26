# The audit log's head held by the journal, and old segments aged out on the record

GAP-163 and GAP-152 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-104, D-105 and D-106 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
DN-23 §14. Human-owned: `gungnir-security`; what the owner has reviewed is in
[`../../signatures.md`](../../signatures.md). D-104 is the owner's own decision of
2026-09-26; D-105 and D-106 were taken under the owner's delegation of that day.

## What was wrong

GAP-111 put the audit record on the disk, one hash-chained segment per run, and said what
the chain could not do: the last lines of a segment could be removed and what was left
still verified, because nothing outside the file knew where it had ended (GAP-163).
Building this found the same limit is wider than a cut. The chain is unkeyed, so anyone
able to write the file can drop its tail and append lines of their own with hashes that
verify. And nothing aged the segments out: `RetentionPolicy::max_audit_log_age_days` still
said it governed nothing, and PN-20 showed only the current run (GAP-152).

## Where the head is kept

The owner decided it: in the event journal. The journal is durable, append-only and sealed
under the deployment's key, and each binary already writes it. The audit entries stay off
it, as D-87 decided, because they would be streamed to every linked desktop; a head is a
file name, a count and a hash, and says nothing of who did what.

Three events, `AuditEvent` in `gungnir-model`:

- **`Anchored`**: this run's segment reached this head. Its cadence (D-104's delegated
  detail) is every 64 entries, or five seconds after the first entry the last head does
  not cover, whichever comes first, and always at close. Five seconds is the desktop
  journal's own sync interval (D-04), so the anchor adds no window the journal does not
  already accept, and a quiet console journals nothing at all. Sixty-four bounds a burst:
  at the node's attributed lane's sustained 200 entries a second (D-87) it is about three
  anchors a second, where one per entry would put 200 events a second on the journal and
  on every linked desktop's stream. **Only lines already on the disk are anchored**: the
  node syncs once a tick and anchors after the sync, and the desktop syncs every entry. A
  crash can leave the journal behind the file, never ahead of it, so a crash never reads
  as a cut.
- **`Verified`**: what a verification found, and the head every earlier segment is
  expected to reach from then on. This inventory supersedes everything before it, so a
  start reads the journal newest first only back to the last one -- normally the previous
  run's session, not the deployment's life. A segment found cut or gone keeps the head it
  should have reached, so it is found again at every start rather than accepted as the new
  baseline after being reported once.
- **`Purged`**: retention removed a segment on purpose.

Because each segment's first line continues the one before, the newest head fixes every
line before it; the per-segment heads are what let a whole removed file be named.

## What verification finds, and who sees it

`gungnir_security::verify_audit_record` checks the chain as before and every segment
against the head the journal holds for it: **cut** when it holds fewer entries than its
head ("audit-000002.jsonl was cut: the journal holds its head at entry 9 and 6 remain, so
3 entries are missing"), **rewritten** when the entry at the head's position is not the
head, **gone** when the journal holds a head for a file that is not there and records no
purge. A segment longer than its last head verifies, and says how many entries follow it:
that is a run stopped after its last anchor, which is the window left.

It runs at start on both binaries, once the live session exists and before retention, and
on the desktop again whenever PN-20's "Verify now" is pressed. What it found goes on every
record: the journal (`Verified`, with the findings as sentences), the audit log itself (one
`audit.anchor_mismatch` entry naming them), the desktop's alerts (one per problem, each
naming its file), PN-20 (the summary in the warning colour and every segment's state), and
the node's log (one error line per problem, and a count on every health line). A journal
session that cannot be read -- sealed under a key the process no longer holds -- is named,
because a head recorded in it was not checked; a deployment journaling under an ephemeral
key therefore cannot check its earlier heads at all, and is told so.

The node has no route that serves its audit record, so a linked desktop's PN-20 cannot show
the node's verification; the node's operator surface is its log. Filed as GAP-179.

## Retention, and why it cannot read as a deletion

D-105. Opt-in exactly as D-78 made the session purge: no policy in the baseline, nothing
purged, and both binaries say which at start. Age is days since the segment was last
written. The purge runs beside the session purge, at start and hourly, **oldest first, and
stops at the first segment it must keep**: the newest (it holds the chain's head, and the
next run numbers its segment after it), this run's own, one under an `audit-NNNNNN.hold`
beside it, and those a run under a held session anchored in that session -- so the audit
record of a session under after-action review is kept with it. A prefix keeps what remains
one unbroken chain, so a purge never reads as a break and a removal in the middle, which
only somebody other than retention makes, still does.

Each removal is renamed out of the listing, **journaled and the journal synced**, recorded
as an `audit.purged` entry in the live segment, and only then deleted. A purge interrupted
before the record leaves a `.purging` file the next purge records and removes; the verifier
reads one as a purge in progress. A `.purging` file the policy would not have purged was
not left by a purge, and is put back and said. The desktop raises an alert naming what was
removed; the node logs it and counts it on its health line.

## PN-20

D-106. The panel lists every segment, earlier runs' included, with its state from the last
verification, and opens any of them read only, its chain checked as it is read. No new
permission: reading the record is what seeing PN-20 already allows, and verifying changes
nothing but the record of having checked. An open segment draws its newest 2,000 entries and
says how many more the file holds.

## Tests

`gungnir-security/src/audit/anchor.rs` and `audit/retention.rs` (a cut named with its count,
a rewritten tail, a file longer than its head, a removed file named and a purged one not,
the fold stopping at the newest inventory; the prefix purge, holds, an interrupted purge
finished and recorded, a stray `.purging` restored, read-back); the cadence tests in
`gungnir-security/src/audit.rs`; `gungnir-app/tests/audit_record.rs` and
`gungnir-node/tests/node_audit_record.rs` on each binary's real paths; and the PN-20 render
test in `gungnir-ui/src/panels/rendered.rs`. The `gungnir-security` row in the
verification table is unchanged and left for the owner's walk.

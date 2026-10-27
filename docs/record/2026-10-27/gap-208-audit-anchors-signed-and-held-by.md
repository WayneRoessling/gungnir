# GAP-208 audit anchors signed and held by the node

Plan 12 tranche B, the first build. The review's §5 finding 1: on a disconnected desktop the
audit chain is unkeyed, and its head is kept in a journal sealed under a key the operator's
own passphrase or OS session unlocks. So the operator who can rewrite the audit file can
reseal the journal with a head to match, and the record cannot find a rewrite by its own
writer. D-142 chose the answer: a deployment `AuditSigning` key the operator's session
never unlocks, and anchors forwarded to the node whenever linked.

## The owner's four choices, 2026-10-27

Each was the recommended default.

| Question | Chosen |
|---|---|
| Who unlocks the desktop's signing key | An administrator's passphrase, typed when the desktop starts |
| Who detects a rewrite on reconnect | The node judges signatures and contradictions; the desktop checks its file against the node's heads |
| While the key is locked | Run, anchors unsigned, and say so |
| The node's own anchors | Not signed: its operator cannot reach them |

## Why custody is the hard part

Any key the desktop process can use while disconnected is, in the end, in a process the
operator runs. A second sealed file opened by the operator's own keystore would change
nothing. So the key is sealed under a passphrase the administrator holds, and an
administrator unlocks it when the desktop starts. That stops the rewrite made with a text
editor and the operator's own keystore, which is the attack the review found. It does not
stop someone who can read the running desktop's memory. The node is the check that does
not depend on the desktop: it holds every head forwarded while linked, where the operator
cannot reach.

## What was built

- **`gungnir-security` (human-owned).** `KeyPurpose::AuditSigning`, and an `audit::signing`
  module. `AuditSigner::provision` makes the key once in `<data dir>/audit-signing/` and
  never replaces it. `AuditSigner::unlock` never creates one. `AuditVerifyingKey` checks a
  head against the baseline's public key. `signing_report` sorts heads into signed,
  unsigned and invalid, and only invalid counts as damage. `SegmentHead` carries the
  signature. `audit-key provision` reads the passphrase from standard input. A new audit
  entry name, `audit.signing_unlock`, records every attempt, opened or refused.
- **`gungnir-model`.** `AuditHead::signature`, `AnchorSignature` with its hex encoding, and
  the node's `AuditEvent::Forwarded` (with an `AnchorVerdict` and any finding) and
  `AuditEvent::ForwardedVerification`. All are additive and defaulted.
- **`gungnir-config`.** `security.audit_signing.public_key_pem`, validated like the escrow
  key: the public half only.
- **`gungnir-api` (human-owned write path).** `POST /v3/audit/anchors` queues a desktop's
  heads under its certificate's name, `202`. `GET /v3/audit/anchors` serves that desktop's
  held heads and no other's. Both ask for a signed-in session; neither asks for a role.
- **`gungnir-node`.** `DesktopAnchors` recovers held heads from the journal at start. Each
  tick it judges forwarded heads' signatures, finds a rewrite (same line count, another
  hash) or a cut (fewer lines than held) from the heads alone, journals each new head, and
  publishes what it holds.
- **`gungnir-remote`.** The link's anchor outbox, flushed each forward tick, and one read of
  the held heads per connection.
- **`gungnir-app`.** The baseline's key is read at start, and the strip says anchors are
  unsigned until unlocked. PN-20 offers the administrator's passphrase field and the
  signing sentence. Every anchor and every inventory head is signed once unlocked. Heads go
  to the link, the whole inventory at start, so a restarted desktop's record reaches the
  node. On reconnect the file is checked against the node's heads, this run's segment
  included, and the findings are alerted, written to the audit log and forwarded.

## Tests

- **`gungnir-app/tests/audit_signing.rs`.** Locked at start and said so; a wrong
  passphrase refused and recorded; the administrator's unlocks; every later anchor
  verifies under the baseline's key. A keystore holding another deployment's key is
  refused. **The tranche B criterion:** a head reaches the node, the file is cut mid-run,
  and on reconnect the cut is found against the node's head and forwarded.
- **`gungnir-api/tests/audit_anchors.rs`, over mutual TLS.** Heads are queued under the
  certificate's name, read back by that desktop alone, refused without a session, and
  refused whole past the size bound.
- **`gungnir-node`.** A rewrite and a cut found from the heads alone, and a head already
  held not journaled again.
- **`gungnir-security`.** Provision, unlock, sign and verify; another deployment's key;
  signed, unsigned and forged heads sorted apart.

## What this does not do

- **Memory.** A key unlocked in the desktop's memory can be used by someone able to read
  that memory. It is stated, not solved.
- **Unsigned heads are not damage.** An honest desktop writes them while the key is
  locked, so verification lists them and does not condemn them.
- **A segment deleted whole is not checked against the node.** The node does not know which
  segments retention purged, so that is left to the local verification.
- **Recovery reads every session.** The node folds every journal session at start to
  recover held heads. A deployment with years of sessions pays for that at start.
- **Human-owned standing.** `gungnir-security` and the `gungnir-api` routes are human-owned.
  What the owner has signed of this change is in `../../signatures.md`.

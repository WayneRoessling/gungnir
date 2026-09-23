# A desktop is named by its own key

GAP-141 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
on the owner's two answers of 2026-09-22.

## What was wrong

Every desktop's link identity carried one common name, `gungnir-app`. A node reading the
party off a handshake, or an `origin` off a decision forwarded after an outage, learned
that the work happened on a desktop and never which desktop. That is enough while nothing
has to tell two of them apart, and GAP-137 is exactly the thing that does: an exchange
register has to keep one producer's set apart from another's, and had nothing to key a
desktop on.

## The decision, and why

Offered a name the deployment states or one derived from the desktop's key, **the owner
chose the key** (2026-09-22). A desktop's certificate is self-signed, so a stated name is
a claim nothing checks and two desktops can make the same one; a fingerprint of the public
half is a function of what the handshake actually proved possession of. The node comparing
a forwarded batch's `origin` with the name on the certificate it verified is then comparing
two views of one key, and needs no configuration to do it.

A desktop is `desktop-` followed by sixteen hex digits of a SHA-256 over the public half.
Sixty-four bits: two desktops collide by accident at about one chance in four billion across
a hundred thousand of them, against the certainty of collision this replaces.

**The ephemeral case is named, not hidden.** A desktop whose key provider is unreachable
gets a fresh key each start. The owner's second answer was that such a desktop still
forwards, and says so: its name is `desktop-ephemeral-` and the same fingerprint, so a
record carrying it says what the origin is worth. Refusing to forward would have kept every
offline decision off the node's record in exactly the configuration that can least afford
it (DN-31 §9 row 8, MOE-11 = 1.0).

## What changed

- `gungnir_remote::identity::desktop_name` derives the name; `DesktopIdentity` carries the
  key, the name and whether it persists. `issue_desktop_outbound_identity` and
  `issue_desktop_identity` return one. `issue_for_client` keeps its stated-name form for
  the node's peer links, which are a node talking as itself.
- **The desktop issues its identity once**, in `AppState`'s construction, and holds it as
  `machine_identity` (`identity` was already a track's identity, GAP-010). `session::link_tls`
  reads it rather than issuing again: on the ephemeral path a second issuance is a second
  key, and the name a node verifies would stop being the name a forwarded batch carries.
  The peer links bound at start use the same identity for the same reason.
- `failover.rs` forwards under `session::origin_of`, which is that name, or
  `desktop-unidentified` when issuance failed and no development certificate stood in
  either -- a case where the link carries nothing and a node sees no party at all.
- **The node checks it.** `POST /v3/decisions/forwarded` refuses `403`, with nothing
  reaching the loop, when a batch names a machine other than the one the handshake
  verified. Where there was no handshake identity there is nothing to compare, and the
  node does not pretend to check.

## Found on the way

`gungnir-api/tests/mutual_tls.rs`'s own certificate authority issued every certificate
under the subject name **`rcgen self signed cert`**. `CertificateParams::new` sets the
subject alternative name and leaves the common name at rcgen's default, so a helper whose
signature reads `issue("desktop-1")` produced a certificate a node reads as
`rcgen self signed cert`. No test noticed, because until now none compared the party with a
name anything else chose. `gungnir_remote::identity::issue` has always set the common name,
so no shipped path was affected; the test authority now sets it too.

## The tests

- `gungnir-remote`: two desktops issued in one process get different names; one key gives
  one name however often it is asked; the persisted and ephemeral forms differ and say
  which they are; and the name on the issued certificate is the name the identity reports,
  which is the agreement the node's check turns on.
- `gungnir-api/tests/mutual_tls.rs`: over a real mutual-TLS connection, a batch naming
  another machine is refused `403` naming both, nothing reaches the loop, and the same
  batch under this machine's own name passes the check.
- `gungnir-app/tests/cut_off_and_reconnected.rs` (DN-31 §9 row 8) now reads desktop A's own
  name rather than a shared constant, and asserts first that A and B do not share one.

## What this does not do

It does not give a desktop a human-readable label. An operator reading a journal sees
`desktop-9f3a61c2ab12cd34`, not "ops console 2". A label mapping names to words is a
deployment's to state and nothing here needs it; the short-tag convention D-61 already puts
identifiers on screen this way.

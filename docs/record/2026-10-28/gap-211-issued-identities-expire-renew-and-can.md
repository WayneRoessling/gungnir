# GAP-211: issued identities expire, renew, and can be revoked

Plan 12 tranche B, the fourth build. The review's §5 finding 4: every desktop and node
identity was valid from 1975 to 4096, rcgen's defaults. The verifier carried no revocation,
and identities are self-signed and pinned. A desktop whose key was taken therefore stayed
trusted until someone edited every client-authority file by hand.

## The owner's four choices, 2026-10-28

| Question | Chosen |
|---|---|
| The validity window | 90 days for every role, against the recommendation of 30 for a desktop and 90 for a node |
| How a key is revoked | A key-fingerprint deny list in the signed baseline |
| When an identity is re-issued | At start, and while running once a third of the window is left |
| What PN-01 shows | This desktop's own identity and its days left, with an alert under seven days |

## What was built

- **`gungnir-security` (human-owned).** `key_fingerprint`, the one definition of a key's
  name. Identity names, the deny list and GAP-210's baseline signer now share it, where
  two copies of the same arithmetic stood before.
- **`gungnir-remote` (the human-owned TLS identity path).**
  - `issue` sets a 90-day window.
  - `IssuedIdentity` holds an issued certificate and re-issues it on the same key in the
    handshake that finds 30 days or fewer left. It is both the client and the server
    certificate resolver.
  - `LinkTls.issued` holds one, and `LinkTls.revoked_keys` is the deny list.
  - `client_config` refuses a server whose key is listed.
- **`gungnir-api`.** `acceptor_with_resolver` and `acceptor_revoking` refuse a client whose
  key is listed before the pinned authorities are consulted. `certificate_key_fingerprint`
  and `refuse_revoked` are the one check both sides call.
- **`gungnir-config`.** `security.tls.revoked_keys`, validated as sixteen lower-case hex
  digits.
- **`gungnir-node`.** Serves through its re-issuing identity, and passes the deny list to
  both acceptors and its peer links.
- **`gungnir-app` and `gungnir-ui`.** The tick keeps the desktop's identity current and
  raises one alert per condition. PN-01's identity element shows the days left.

## Why renewal is in the handshake

A timer would need a task kept alive in both binaries and a way to swap the certificate
into configurations already built. A resolver is consulted on every handshake anyway, so
the check costs one comparison and the swap is a field assignment under a lock. The key
does not change, and rustls reduces a pinned certificate to its name and key, so no peer
re-pins. The desktop also asks once a tick, because a desktop that makes no connection for
weeks would otherwise reach its last week before the first handshake that renews it.

## Tests

- `gungnir-remote`, unit tests, against rustls's own verifier:
  - a new identity has 89 or 90 days left, verifies now, and is refused past its end;
  - it is not re-issued at 31 days left, and is at 29, with the same name and key;
  - the re-issued certificate verifies against the first one's pin after the first one's
    end;
  - the date arithmetic gives the calendar dates.
- `gungnir-remote/tests/revoked_keys.rs`, over TCP with the real acceptor and client:
  - pinned identities link when nothing is listed;
  - a node refuses a client whose key it lists, though that client is pinned;
  - a client refuses a node whose key it lists, though that node is its trust root.
- `gungnir-config`: the list's form is validated.
- `gungnir-ui`: PN-01 draws each state of the identity element.
- `gungnir-app/tests/machine_identity_expiry.rs`: a desktop's new identity raises nothing,
  and the alert wording at five days left and past the end.

## What this does not do

- **A window does not bound a stolen key.** Whoever holds the key can issue a certificate
  that verifies against the same pin. Revocation is by key for that reason.
- **The deny list takes effect at a restart**, on the hosts whose baseline carries it.
- **No command rotates a key.** A revoked machine is given a new one by starting it without
  its identity keystore file, as `deploy/README.md` says.
- **Human-owned standing.** What the owner has signed of this change is in
  `../../signatures.md`.

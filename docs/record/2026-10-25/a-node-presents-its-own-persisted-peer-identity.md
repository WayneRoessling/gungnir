# A node presents its own persisted peer identity

GAP-060, closed to D-135 (the owner's decision of 2026-10-25). Record items 105 and 111
left one question open: what the node presents when it connects out to a peer. Until now
that was a certificate made fresh at every start, self-signed and named `gungnir-node`.
That is the same name every node's serving certificate carried, too.

## What was wrong, found before the decision and confirmed building it

- **A peer could never pin it.** A peer verifies a client certificate against a fixed
  client-authority file (`gungnir_api::tls::acceptor_with_key`), and a certificate that
  changes at every start cannot be in that file ahead of time.
- **The operator's certificate was read but never presented.** `client_config` prefers
  the issued identity over the configured PEM. Issuing the ephemeral identity always
  succeeded, so `GUNGNIR_TLS_CERT`/`GUNGNIR_TLS_KEY` never reached a peer link.
- **Nodes could not be told apart.** Every node's outbound and serving certificates were
  named `gungnir-node`, so a peer's `machine_identities` had nothing to tell one node from
  another by.
- **No certificate stated its role.** None carried an extended key usage.
- **Found building it: the link refused the issued identity outright.**
  `link::start_as_machine` checked only `identity_pem`, not `LinkTls::has_identity`. So a
  node with nothing configured never even started a peer link. Taken with the first two
  points, a node's peer link over mutual TLS could not have worked in any configuration.
  No test covered a node-to-node peer link over mutual TLS, which is how all of this went
  unseen.

## What was built

- **`gungnir-remote::identity`, human-owned.**
  - **`Usage`.** Every certificate now states its one role as its extended key usage:
    `serverAuth` on a serving identity, `clientAuth` on a desktop's and on a node's
    outbound one (RFC 5280). `issue` takes it as a new argument.
  - **`node_name`.** A node's names are derived from its key, as a desktop's are
    (GAP-141): `node-<fingerprint>` for serving and `node-peer-<fingerprint>` for
    outbound, with `-ephemeral-` when the key does not survive a restart.
    `issue_node_serving_identity` no longer takes a name.
  - **The node's peer identity.** `issue_node_peer_identity` is the node's own key,
    persisted under its own keystore service (`gungnir-node-peer-tls-identity`) and its own
    file (`peer-tls-identity/`). It never shares a key with the serving identity: one key
    per purpose (NIST SP 800-57 Part 1). `issue_node_peer_identity_ephemeral` is the
    unpersisted form.
  - **Supporting changes.** `issue_for_server` is the serving counterpart of
    `issue_for_client`, for harnesses. `HostIdentity` now carries its name and whether it
    is persisted.
- **`gungnir-node`, `host_tls`.**
  - **The configured certificate wins.** When `GUNGNIR_TLS_CERT` and `GUNGNIR_TLS_KEY`
    are both set and readable, that certificate is presented and nothing is issued. One
    that is set but unreadable is logged as an error, and the node falls back to its own
    identity rather than to none.
  - **Otherwise, the node's own peer identity.** It is persisted when the baseline uses
    the operating system's keystore, the same gate a desktop's identity has, and its
    public certificate is written as `node-peer-identity.pem` beside the journal. It is
    ephemeral, and named so, when the baseline doesn't use the keystore.
- **`gungnir-remote::link::start_as_machine`.** It now accepts either identity.
- **The tests' harnesses.** `cut_off_and_reconnected.rs`'s partner route served under a
  client identity, which a key usage now forbids, and serves under `issue_for_server`.
  Three other harnesses pass the new argument.

## What operators do

- **Re-pin each node once.** Its serving certificate's name changes from `gungnir-node`
  to `node-<fingerprint>`.
- **Pin each node's peer identity at the peers that accept it.** The certificate is
  `node-peer-identity.pem`, and the peer's agreement and `machine_identities` name it as
  `node-peer-<fingerprint>`.
- **Or configure a certificate.** A deployment with its own certificate authority sets
  `GUNGNIR_TLS_CERT`/`GUNGNIR_TLS_KEY`, and that is what peers see.

## Not decided here

A deployment certificate authority issuing every machine's certificate is the fuller
industry pattern: short-lived certificates, rotation, and revocation. That is a custody
decision under D-02 and D-29. Because the configured certificate now wins, that path stays
open.

## Checks

- **The identity module's unit tests.**
  - **Key usage.** Every certificate states its one role and never the other.
  - **Names.** Two nodes, and a node's two roles, never share a name. The name on the
    certificate is the one reported, and it is stable for one key.
  - **Fallback.** The peer identity falls back honestly when the keystore is unreachable.
  - **Separation.** The five keystore services and the node's two keystore files are
    distinct.
- **`gungnir-remote/tests/tls_link.rs`, two new tests over real mutual TLS.**
  - **A node links to a peer node.** The peer serves under its serving identity and pins
    the connecting node's peer identity. The link comes up, the agreement matches it by
    its `node-peer-` name, and the agreed track arrives.
  - **Roles are enforced.** The same setup with a serving certificate presented as a
    client is refused at the handshake. The only difference is the key usage.
- **`gungnir-node`'s `host_tls` test.** The configured certificate wins, an unreadable one
  falls back to the node's own identity, and with nothing configured the node presents
  its own `node-peer-ephemeral-` identity.

## Ownership

`gungnir-remote/src/identity.rs` is human-owned: it is the TLS identity path
(`docs/agentic-workflow.md`). `docs/signatures.md` records its signature. The change to
`link.rs` and the node's `host_tls` are outside that path.

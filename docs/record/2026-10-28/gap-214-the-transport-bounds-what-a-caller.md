# GAP-214: the transport bounds what a caller can hold

Plan 12 tranche B, the seventh build, for the review's §5 finding 11. The event stream
upgraded before any credential and then waited for its subscribe frame with no deadline,
so a peer holding a valid certificate could hold sockets open indefinitely. Request bodies
were bounded only by the framework's default, and TLS 1.2 was still negotiable.

## What was found on the way

**Handshakes are taken one at a time.** `TlsListener::accept` completes each TLS handshake
before accepting the next connection, so a client that opened a socket and never spoke held
up every connection behind it, not only its own. This was not in the finding, and the
handshake deadline below is what bounds it.

## What was built

All of it is in `gungnir-api`'s transport and listener, and in `gungnir-remote`'s client
configuration. No crate was added: axum's `DefaultBodyLimit` and middleware, tokio's
timeout and semaphore, and rustls's version selection cover it.

| Limit | Value |
|---|---|
| TLS versions, node and client | 1.3 only (`tls::server_config`, `client_config`) |
| Connections open at once | 256 (`MAX_CONNECTIONS`); past it a connection waits in the backlog |
| Handshake | 10 s (`HANDSHAKE_DEADLINE`) |
| Upgrade to subscribe frame | 10 s (`SUBSCRIBE_DEADLINE`), then the stream is told why and closed |
| Any request | 30 s (`REQUEST_DEADLINE`), then `503` |
| Request body | 4 MiB (`MAX_BODY_BYTES`), twice the framework's old default |

The body limit is above the old default so that nothing accepted before is refused now. The
largest bounded body, an audit-anchor inventory, is about a third of it.

## Decisions taken under the owner's delegation of 2026-10-28

- **The values above.** A node serves tens of desktops and partners. Each deadline is long
  enough for a slow link and short enough that a stalled caller frees its place well
  within a minute.
- **The connection cap is on the TLS listener only.** The plaintext listener refuses any
  address but loopback, so whoever could exhaust it is already on the host.
- **No new crate.** `tower`'s limits would have needed a §2.9 row, for what a semaphore
  in the listener does.

## Tests

`gungnir-remote/tests/transport_limits.rs`, over real sockets and the real acceptor and
router:
- **TLS 1.2 refused.** A client offering only TLS 1.2, with the same trust and the same
  pinned identity as a served 1.3 client, is refused on the version.
- **Stalled handshake.** A connection that never handshakes holds the next client up for
  the deadline and no longer.
- **No subscribe frame.** A stream that never subscribes is told why and closed.
- **Oversized body.** A body one byte over the limit is answered `413`.

The request deadline has no test of its own: no route can be made to stall without adding
one for the purpose.

## What this does not do

- **Handshakes are still taken one at a time.** The deadline bounds how long one client
  can hold up the rest. Taking them concurrently is a larger change to the listener.
- **The FIPS provider question is GAP-230's**, as the gap said.

# gungnir-api v1: interface control document (draft)

The external contract of a Gungnir service node (`ARCHITECTURE.md` §8). Desktops
in the connected profiles, peer command-and-control systems, analytics tools, and
enterprise services integrate through this interface and nothing else. The types
are defined in `gungnir-api/src/v1/mod.rs` and reuse `gungnir-model` views and
`gungnir-eventing` envelopes, so the API can never disagree with the desktop about
what a track or a plan is.

Status: the types exist and are covered by serialization tests; the transport crates
were signed off on 2026-09-05 (D-18, `agentic-coding-standards.md` §2.9) and are in the
workspace manifest, but no code uses them yet. `gungnir_api::ApiServer::serve` returns
`TransportNotImplemented` until it does, and will keep saying so rather than reporting a
server that is not listening.

## Transport decision

JSON over HTTP for request/response, and a WebSocket carrying the event stream.
Rationale: both desktop clients and peer systems can consume them without code
generation, they are trivially testable with recorded fixtures, and the JSON shapes
are exactly the `serde` forms the journal already stores. gRPC is planned as a
second transport for peer C2 systems that require it, using the same message
types, and is not part of the 2026-09-05 sign-off.

The crates are `axum` (with its `ws` feature, which is the server side of the WebSocket),
`tokio-tungstenite` for the desktop's client, `reqwest` for the desktop's HTTP client, and
`rustls` with `tokio-rustls` (PEM reading is rustls's own `pki_types::pem`, which
replaced `rustls-pemfile` on 2026-09-07) for the mutual TLS that D-02 requires of
machine identities. `agentic-coding-standards.md` §2.9 records why each was chosen and
which pins keep a single copy of each in the binary.

## Versioning

- Every payload carries `schema_version` = `gungnir_model::SCHEMA_VERSION`; a client
  refuses data from a node whose version it does not know.
- The URL path is versioned (`/v1/...`). `v2` is added alongside, never in place
  of, `v1`; `v1` is removed only after every known client has moved.
- Schema names and versions for interop formats are governed by
  `gungnir_interop::SchemaCatalog`.

## Version 2, decided 2026-09-05

The plan type changes shape so that a plan can be an intercept or a fires task:
`PlanView.solutions` becomes `PlanView.kind: PlanKind`
(`design/DN-05-fires.md`, `design/model-and-schema-deltas.md` §3). That changes an
existing field's type, which the compatibility rules below say needs a new schema version
and a new path version.

The owner decided on 2026-09-05 to **replace rather than mirror**:

- `gungnir_model::SCHEMA_VERSION` goes from 1 to 2.
- The path goes from `/v1` to `/v2`, and `/v1` is removed rather than kept alongside.
- `solutions` is not retained as a deprecated field.

Removing `/v1` is allowed by the rule above rather than an exception to it: `v1` is
removed once every known client has moved, and the set of known clients is empty. The
transport is not in the workspace and `ApiServer::serve` still returns
`TransportNotImplemented`, so nothing is deployed against `/v1`.

The endpoints plan 11 adds land under `/v2` for the same reason: the model change and the
new endpoints arrive together rather than in two migrations. They are listed in
`design/model-and-schema-deltas.md` §7 with their authorization actions.

**Landed 2026-09-05.** `gungnir_model::SCHEMA_VERSION` is 2, the module is
`gungnir-api/src/v2/`, and `API_VERSION` reads `v2`.

**`SCHEMA_VERSION` went to 3 on 2026-09-06** when `DetectionView.measurement` became
`gungnir_model::Measurement` (`design/DN-27-bearing-only-detections.md` §4 and §8).

**The path did not move, and the rule was amended so that it need not.** The owner
resolved this on 2026-09-06 by changing the rule rather than the path: a payload change
now requires a new path version only where a client that does not know about it could
silently misinterpret something, and a detection posted in the previous shape is not that
case, because the caller is refused by name.

**One claim made here before that amendment was false and is corrected.** This paragraph
used to say such a client "is refused by the `schema_version` check rather than by the
path". There was no `schema_version` check on that path, or on any inbound path: the
constant existed, the function to check it existed, and **nothing called it**. The client
was refused only because serde could not read the old array as the new enum -- an accident
of this particular change. The guard was built before the rule was relaxed, and the rule
now depends on it explicitly. Nothing else in the interface is affected -- the endpoints
that publish carry tracks, plans, assets and health. All three moved in the same change as
`PlanKind`, because bumping the version first would have made every payload claim a shape
it did not have. This document keeps its filename so the doc-comment citations that point
at it stay correct; its content describes v2.

## Authentication and authorization

Every request carries a credential that `gungnir_security::Authenticator` resolves
to an `OperatorId`; every handler checks `gungnir_security::Authorizer` for the
action in `gungnir_security::actions` before doing anything. Unauthorized calls
return `ApiError::Security`. The credential mechanism (mutual TLS or bearer
tokens) is an open decision in `ARCHITECTURE.md` §8.5.

## Endpoints

The paths are `/v2`; the table said `/v1` until 2026-09-05, which the "Version 2"
section above had already superseded.

| Method and path | Request | Response | Authorization action | Built? |
|---|---|---|---|---|
| `POST /v2/session` | `SessionRequest { operator, passphrase }` | `SessionResponse { token, expires_s }` | none: this is what establishes identity | Yes (GAP-057) |
| `GET /v2/session` | none | `SessionStatus { operator, role, expires_s }` | a valid token | Yes (GAP-057) |
| `GET /v2/snapshot` | none | `SnapshotResponse { schema_version, tracks, plan, health, requirements, bearing_rays, pipeline_stats }` | `picture.view` | Yes (GAP-041); `bearing_rays`/`pipeline_stats` GAP-096 |
| `GET /v2/events` (WebSocket) | `SubscribeRequest { from_seq }` as the first frame | A stream of `EventFrame` (`gungnir_eventing::Envelope`) with `seq >= from_seq`, in order | `picture.view` | Yes (GAP-041) |
| `GET /v2/health` | none | `SystemHealth` | `picture.view` | Yes (GAP-041) |
| `GET /v2/coverage` | none | `CoverageResponse`: the whole `CoverageReport` when one was computed, or `NotComputed` with a reason. **Not a bare `Vec<CoverageGap>`**, which would discard the sample spacing and terrain-masking flag DN-12 §5 puts on the result | `picture.view` | Yes (GAP-006) |
| `POST /v2/detections` | `SubmitDetectionRequest { detection: DetectionView }` | **`202`**: queued for the ingest gateway, which validates it on its next tick; `IngestEvent::Quarantined` appears on the stream if it is rejected | `detection.submit` | Yes (GAP-057) |
| `POST /v2/plans/{plan_id}/decision` | `ApprovalRequest { plan, accepted, operator }` | `204`; a `CommandEvent::Decided` appears on the stream | `plan.decide` (or `plan.override`) | **No: `501`, because a node runs no approval queue** |

**Authentication landed the same day (GAP-057, DN-23 §6).** Every route but
`POST /v2/session` requires a bearer token the node minted; the event stream carries its
token in the subscribe frame, because a WebSocket client cannot always set a header on the
upgrade. `ApprovalRequest` names an operator in its body and **that field is not
believed**: the caller is whoever the token says. A node with no account store configured
answers `503` on every route, saying it authenticates nobody -- which is the default
deployment, and better than serving the picture to anyone who asks.

**Why one write path still refuses.** `POST /v2/plans/{plan_id}/decision` returns `501`
because **a node runs no approval queue**: plans are routed through the policy chain and
the queue on a desktop, and a node publishes `PlanProposed` and stops. Serving it would
mean inventing a queue in a request handler. That is a different reason from the one below,
which applied before there was any authentication at all.

**Why the write paths refused before 2026-09-05.** The authentication section above says every
request carries a credential that `gungnir_security::Authenticator` resolves to an
`OperatorId`. That trait exists, in `gungnir-security/src/authn.rs`, and **has no
implementation** -- nothing in the workspace turns a credential into an identity. The
authorization half is real (`Authorizer`, `role_permits`, the action names); the
authenticating half is a trait and a comment saying the mechanism is not yet chosen, which
D-02 has since chosen: operator tokens are GAP-057 and machine identity is GAP-060. Since
`ApprovalRequest` names its own operator in the body, serving it would let any caller
accept a plan as anybody. Both routes exist and return `501` with that reason, rather than
`404`, which would wrongly say they are not part of v2.

**Why only loopback is served.** There is no TLS (GAP-060, which waits on GAP-084's key
custody), so `gungnir_api::transport::serve` refuses any bind address that is not
loopback, and the client refuses an `https` endpoint rather than downgrading it.

**Retention for `from_seq`** is a bounded in-memory window on the node
(`transport::BACKLOG_CAPACITY`), which is smaller than the journal's. The behaviour the
rule below describes is unchanged: too far back is an error and the client takes a fresh
snapshot.

Error responses carry `ApiError` as `{ "error": "<variant>", "message": "<text>" }`.

## Event stream semantics

- Envelopes are delivered in `seq` order; a gap means the client missed events and
  should re-request from its last applied `seq`.
- A client that supplies a `from_seq` older than the node's journal retention
  receives an error and must take a fresh snapshot.
- The node is the system of record; a desktop applies envelopes to its projection
  (`gungnir_collab::SharedPictureSync`) and never treats its own state as
  authoritative while connected.
- An idle stream is kept alive by a server ping every
  `transport::HEARTBEAT_INTERVAL` (2 s, D-23), and a client that receives nothing at all
  for `transport::HEARTBEAT_TIMEOUT` treats the link as gone and reconnects. The timeout
  is derived from the interval -- three tolerated misses plus a one-second margin, 7 s --
  rather than being a constant of its own, so the two cannot drift apart. Without the
  heartbeat a half-open connection would leave a desktop showing a dead node's picture
  under a "connected" light; with it, a desktop also shows how long ago the node was last
  heard, which is the number that tells an operator how stale the picture is.

## Compatibility rules

- Adding a field with a default is compatible.
- Removing or renaming a field, or changing a type, requires **a new schema version**.
- It requires **a new path version** only where a client that does not know about the
  change could **silently misinterpret** a payload. Where such a client is cleanly
  refused, the schema version alone is enough.

  **Amended 2026-09-06, and what it replaced.** The rule used to require both, always.
  That made every change to one payload a migration for the whole interface: every route
  moves, every client re-points, for a field on one message. The cost was real and the
  benefit was not, because the path move's whole value is telling a client something is
  wrong -- and a version check tells it better. A missing route says "there is nothing
  here"; a version check says "you speak 2, this node speaks 3".

  **The condition is not decoration, and it had teeth the moment it was written.** A
  client is "cleanly refused" only where a version check on that path refuses it by name.
  It is *not* enough that its payload happens to fail to deserialise: that is an accident
  of the particular change -- true when an array becomes an enum, false when a field is
  merely widened -- and it answers "the body did not decode", which sends the reader to
  look for a malformed message rather than an old client.

  **When this was written, that refusal did not exist on any inbound path.** The outbound
  direction had one: a desktop compares a node's snapshot version with its own and refuses
  the whole picture. Every write path had nothing, and `gungnir_model::check_schema_version`
  existed with no caller anywhere in the workspace. So the guard was built before the rule
  was relaxed: `SubmitDetectionRequest` carries a `schema_version`, and
  `gungnir-api/tests/machine.rs` proves a caller ahead, a caller behind, **and a caller
  that states no version at all** are each refused with both versions named. That third
  case is the one that matters: the field defaults to zero rather than to the current
  version, because defaulting to current would make every client written before the field
  existed silently claim to be current, which is the opposite of what it is for.

  **A path version therefore still moves for**: a route whose meaning changes while its
  shape does not, a field whose units or frame change without its type changing, and any
  change a client could read as valid and act on wrongly. Those are the silent ones, and
  no version check catches them.
- Enum variants may be added; clients must treat unknown variants as "ignore this
  event" rather than fail.
- Interface conformance is verified by the `Cross-layer` rows in
  `verification-capability-table.md` §2 once the transport exists.

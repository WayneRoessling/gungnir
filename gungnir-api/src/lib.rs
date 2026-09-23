// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! System-of-systems API boundary, per docs/gungnir-capabilities.md §5.5 and
//! ARCHITECTURE.md §8. This crate is the service node's only external surface and
//! the contract the desktop's `gungnir-remote` backends speak. It wraps the same
//! `gungnir-model` views and `gungnir-eventing` envelopes the desktop UI uses,
//! rather than defining a second, divergent contract.
//!
//! Transport (docs/gungnir-api-v1.md, D-18): JSON over HTTP for request/response and a
//! WebSocket event stream, with gRPC as a later second transport for peer C2 systems.
//! [`transport`] implements it on `axum` as of GAP-041. **The read paths and four write
//! paths are served**: submitting a detection, tasking a sensor, reporting on a handoff,
//! and acknowledging a warning, each authorising its caller as either a machine whose
//! certificate speaks for the right role or an operator holding the right action. This
//! sentence previously read "the write paths refuse, because nothing can authenticate a
//! caller yet", which stopped being true when GAP-057 landed the operator session and
//! GAP-041 the machine identity, and was still being said. **Since GAP-132 the node
//! decides too**: it holds the queue (D-55), `GET /v3/queue` serves it and
//! `POST /v3/queue/{item}/decision` takes a person's decision, each handed to the node
//! loop and answered from it rather than acted on in a request handler. `/v3` serves no
//! plan-keyed decision route; the retired `/v2` one names the queue route as its
//! successor. **Since GAP-134 it takes an outage's decisions too**:
//! `POST /v3/decisions/forwarded` carries what a desktop decided while it was cut off,
//! handed to the node loop like the decision route and put on the node's record once.
//! [`UnimplementedServer`] remains for a node that serves nothing at all.

pub mod tls;
pub mod transport;
pub mod v3;

use gungnir_security::OperatorId;

/// The interface version every route is served under.
///
/// **The one constant the node's routes and the desktop's URLs are both built from**
/// ([`path`]), so the two cannot come to disagree about where a route is (GAP-130). It was
/// `v2` until 2026-09-17, when decision, plan and queue-item identifiers became UUID v7
/// written as strings (D-56, D-60) and every payload carrying one changed with them.
pub const API_VERSION: &str = "v3";

/// The version retired on 2026-09-17. Its routes stay routed: each authenticates its
/// caller as its successor does and answers `410 Gone` naming the successor
/// (`transport::router`; `docs/design/DN-31-node-approval-queue.md` §5.1).
pub const RETIRED_API_VERSION: &str = "v2";

/// A route's path under [`API_VERSION`]: `path(routes::SNAPSHOT)` is `/v3/snapshot`.
#[must_use]
pub fn path(route: &str) -> String {
    format!("/{API_VERSION}{route}")
}

/// Every route's path below the version, shared by `transport::router` and the desktop's
/// client in `gungnir-remote`. A path parameter is written as axum writes it; a client
/// that fills one in builds from the prefix before it (`SENSORS`).
pub mod routes {
    pub const SESSION: &str = "/session";
    pub const SNAPSHOT: &str = "/snapshot";
    pub const HEALTH: &str = "/health";
    pub const COVERAGE: &str = "/coverage";
    pub const EVENTS: &str = "/events";
    pub const HISTORY: &str = "/history";
    pub const DETECTIONS: &str = "/detections";
    /// The prefix of [`SENSOR_TASK`], for a client that fills in the sensor.
    pub const SENSORS: &str = "/sensors";
    pub const SENSOR_TASK: &str = "/sensors/{sensor_id}/task";
    pub const HANDOFF_REPORT: &str = "/handoffs/{decision_id}/report";
    pub const WARNING_ACKNOWLEDGE: &str = "/warnings/{asset_id}/{track_id}/acknowledge";
    pub const EXCHANGE_WARNINGS: &str = "/exchange/warnings";
    pub const EXCHANGE_REPORTS: &str = "/exchange/reports";
    pub const EXCHANGE_HANDOFFS: &str = "/exchange/handoffs";
    /// The node's approval queue (DN-31 §7, GAP-132).
    pub const QUEUE: &str = "/queue";
    /// One queue item's decision. **The decision route since GAP-132**: a decision is
    /// taken on a queue item rather than on a plan, because the item is what carries the
    /// deadline and the roles it is offered to.
    pub const QUEUE_DECISION: &str = "/queue/{item}/decision";
    /// The decisions a desktop took while it was cut off from this node, forwarded when
    /// the node answers again (DN-31 §6.8 and §7, GAP-134). New in `/v3`, so it has no
    /// retired `/v2` form.
    pub const DECISIONS_FORWARDED: &str = "/decisions/forwarded";
    /// Retired. Served under `/v2` alone, answering `410 Gone` and naming
    /// [`QUEUE_DECISION`] as its successor; `/v3` never served it (GAP-132).
    pub const PLAN_DECISION: &str = "/plans/{plan_id}/decision";
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Security(#[from] gungnir_security::SecurityError),
    #[error("API transport is not implemented in this build")]
    TransportNotImplemented,
    #[error("request rejected: {0}")]
    BadRequest(String),
    /// The transport could not start or could not carry a message.
    #[error("transport failure: {0}")]
    Transport(String),
    /// A bind address that is not loopback, with no TLS to protect it.
    ///
    /// An error rather than a warning: a node that logged this and carried on would
    /// still be listening. Encryption in transit is GAP-060, which waits on GAP-084's
    /// key custody.
    #[error(
        "refusing to serve {0} in plaintext: only loopback is served until there is TLS (GAP-060)"
    )]
    UnprotectedBind(std::net::SocketAddr),
    /// No credential could be resolved to an `OperatorId`.
    ///
    /// Returned when a caller presents no usable credential. **This is no longer the
    /// ordinary answer from a write path**: operator sessions landed with GAP-057 and
    /// machine identities with GAP-041, and the write paths authorise rather than refuse.
    /// It remains what a node with no caller authority configured says, which is a
    /// deployment that nobody can sign in to.
    #[error("the caller could not be authenticated: {0}")]
    NotAuthenticated(String),
    /// A register that admits no more producers (GAP-137).
    ///
    /// The exchange register holds one set per writer and bounds how many it keeps. A
    /// writer already in it always writes; a new one beyond the bound is refused with
    /// this, because evicting a set a partner is being served from would lose data
    /// silently and refusing leaves the batch queued where the backlog is visible.
    #[error("no room: {0}")]
    NoRoom(String),
}

/// The transport-neutral request handlers a node implements. Every call names the
/// authenticated caller so authorization is enforced per request.
pub trait ApiHandler: Send + Sync {
    fn snapshot(&self, caller: OperatorId) -> Result<v3::SnapshotResponse, ApiError>;
    fn submit_detection(
        &mut self,
        caller: OperatorId,
        request: v3::SubmitDetectionRequest,
    ) -> Result<(), ApiError>;
    fn decide(&mut self, caller: OperatorId, request: v3::ApprovalRequest) -> Result<(), ApiError>;
}

/// A version boundary exists specifically so a later version can be added without
/// breaking the clients of this one -- contract-compatibility governance per the
/// capability description, not just an implementation convenience.
pub trait ApiServer: Send + Sync {
    fn serve(&mut self) -> Result<(), ApiError>;
}

/// Placeholder server that refuses to start, so a node can be configured with a
/// bind address today and fail loudly rather than silently serve nothing.
#[derive(Debug, Clone)]
pub struct UnimplementedServer {
    pub bind_addr: String,
}

impl ApiServer for UnimplementedServer {
    fn serve(&mut self) -> Result<(), ApiError> {
        Err(ApiError::TransportNotImplemented)
    }
}

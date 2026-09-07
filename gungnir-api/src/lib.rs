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
//! GAP-041 the machine identity, and was still being said. `POST /v2/plans/{id}/decision`
//! is the one write path that refuses, and it refuses **architecturally** rather than for
//! want of authentication: a node runs no approval queue. [`UnimplementedServer`] remains
//! for a node that serves nothing at all.

pub mod tls;
pub mod transport;
pub mod v2;

use gungnir_security::OperatorId;

pub const API_VERSION: &str = "v2";

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
}

/// The transport-neutral request handlers a node implements. Every call names the
/// authenticated caller so authorization is enforced per request.
pub trait ApiHandler: Send + Sync {
    fn snapshot(&self, caller: OperatorId) -> Result<v2::SnapshotResponse, ApiError>;
    fn submit_detection(
        &mut self,
        caller: OperatorId,
        request: v2::SubmitDetectionRequest,
    ) -> Result<(), ApiError>;
    fn decide(&mut self, caller: OperatorId, request: v2::ApprovalRequest) -> Result<(), ApiError>;
}

/// A version boundary exists specifically so `v2` can be added later without
/// breaking `v2` clients -- contract-compatibility governance per the capability
/// description, not just an implementation convenience.
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

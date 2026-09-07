//! A link to a partner's node as a machine (GAP-009, DN-16; GAP-065, DN-18).
//!
//! The same link task the desktop uses, started without a sign-in: the client
//! certificate is the identity (D-02), and the partner serves what its exchange
//! agreement for this party lists. What arrives is handed to the host's peer adapter
//! (`gungnir_ingest::adapters::peer::PeerSourceAdapter`) as tracks, which the gateway
//! admits under the peer's own source id with the quality this deployment assigns --
//! never the partner's. This crate does not depend on `gungnir-ingest`, so the host
//! wraps a `PeerLink` in the adapter's stream trait itself.
//!
//! **Two kinds of thing arrive, and they are taken separately** (DN-16 §5): tracks
//! through [`PeerLink::take_tracks`], and launch warnings through
//! [`PeerLink::take_launch_warnings`]. A launch warning is a statement about the future
//! with no kinematic state, so it is not a track and cannot become one -- "a track we
//! have not observed is a track we cannot maintain". Two calls rather than one enum
//! because a caller that only knew about tracks keeps compiling and keeps being right,
//! and a caller that wants the warnings has to say so.

use crate::link::{self, NodeLink};
use crate::{RemoteEndpoint, RemoteError};
use gungnir_model::TrackView;

/// A machine link to one partner.
#[derive(Debug, Clone)]
pub struct PeerLink {
    link: NodeLink,
    endpoint: String,
}

impl PeerLink {
    /// Start the link. Returns at once; `connected` says when the partner has answered.
    ///
    /// # Errors
    ///
    /// As [`link::start_as_machine`]: a URL the link cannot speak to, or no identity.
    pub fn connect(
        endpoint: &RemoteEndpoint,
        handle: &tokio::runtime::Handle,
    ) -> Result<Self, RemoteError> {
        let link = link::start_as_machine(endpoint, handle)?;
        Ok(Self {
            link,
            endpoint: endpoint.url.clone(),
        })
    }

    /// A peer link no task drives, whose projection a test sets by hand.
    ///
    /// The same escape hatch [`NodeLink::scripted`] is: a host that binds peers has to be
    /// testable without a partner on a socket, and a test that stood a real node up to
    /// check an alert's wording would be testing the transport again. It reports not
    /// connected, because nothing has answered.
    #[must_use]
    pub fn scripted(endpoint: impl Into<String>) -> Self {
        Self {
            link: NodeLink::scripted(),
            endpoint: endpoint.into(),
        }
    }

    /// Every track the partner has published or changed since the last call.
    #[must_use]
    pub fn take_tracks(&self) -> Vec<TrackView> {
        self.link.take_changed_tracks()
    }

    /// Every launch warning the partner has issued since the last call (DN-16 §5).
    ///
    /// Returned as the partner published it, with none of this deployment's own stamps
    /// on it: the peer name we know it by and the time we took it are added where the
    /// baseline that names the peer is in scope, which is
    /// `gungnir_ingest::adapters::peer::PeerSourceAdapter`. That keeps the rule DN-16 §5
    /// calls the most important -- what is ours is assigned by us -- in one place for
    /// both quality and identity.
    #[must_use]
    pub fn take_launch_warnings(&self) -> Vec<gungnir_model::LaunchWarningReport> {
        self.link.take_launch_warnings()
    }

    /// Launch warnings the link dropped because its queue was full (DN-16 §5).
    #[must_use]
    pub fn launch_warnings_dropped(&self) -> u64 {
        self.link.launch_warnings_dropped()
    }

    #[must_use]
    pub fn connected(&self) -> bool {
        self.link.connected()
    }

    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.link.read().and_then(|p| p.last_error.clone())
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    #[must_use]
    pub fn link(&self) -> &NodeLink {
        &self.link
    }
}

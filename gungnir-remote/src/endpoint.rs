//! The generic endpoint transport (GAP-040, D-08): one HTTP client that posts a JSON
//! message to a configured address and reports what the endpoint said, off the render
//! thread.
//!
//! **What the caller learns is what the endpoint did**, in three states that are never
//! collapsed: `Accepted` (a 2xx), `Refused` (any other status, with the body), and
//! `Unreachable` (no answer at all). The desktop's store-and-forward rule turns the third
//! into a retry; the second is the endpoint's decision and stands. Nothing here decides
//! what a handoff or a warning means; it carries bytes and reports.
//!
//! Trust roots come from the baseline inline as PEM (GAP-060): a public certificate is
//! not key material, and inline rather than by path keeps a path-to-something out of
//! the baseline. Without roots the client trusts the platform's store, which is right
//! for a development endpoint and wrong for a deployment, and the health line says
//! which is in force.

use std::sync::mpsc;
use std::time::Duration;

use crate::RemoteError;

/// What the endpoint did with the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Accepted { status: u16 },
    Refused { status: u16, body: String },
    Unreachable { reason: String },
}

/// A delivery in flight. Polled on the tick; never blocks.
#[derive(Debug)]
pub struct PendingDelivery {
    rx: mpsc::Receiver<DeliveryOutcome>,
}

impl PendingDelivery {
    /// The outcome, once there is one. A task that ended without reporting is
    /// `Unreachable`, because that is what the caller can act on.
    pub fn poll(&self) -> Option<DeliveryOutcome> {
        match self.rx.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(DeliveryOutcome::Unreachable {
                reason: "the delivery task ended without reporting".into(),
            }),
        }
    }
}

/// The client. One per desktop, built at start with the baseline's trust roots.
#[derive(Debug, Clone)]
pub struct EndpointClient {
    handle: tokio::runtime::Handle,
    client: reqwest::Client,
    /// Whether the baseline supplied roots, for the health line.
    pub pinned_roots: usize,
}

impl EndpointClient {
    /// # Errors
    ///
    /// `RemoteError::Client` when a root certificate does not parse or the client
    /// cannot be built.
    pub fn new(
        handle: tokio::runtime::Handle,
        trust_roots_pem: &[String],
        timeout: Duration,
    ) -> Result<Self, RemoteError> {
        let mut builder = reqwest::Client::builder().timeout(timeout);
        for (i, pem) in trust_roots_pem.iter().enumerate() {
            let cert = reqwest::Certificate::from_pem(pem.as_bytes())
                .map_err(|e| RemoteError::Client(format!("trust root {i} does not parse: {e}")))?;
            builder = builder.add_root_certificate(cert);
        }
        let client = builder.build().map_err(|e| {
            RemoteError::Client(format!("the endpoint client could not be built: {e}"))
        })?;
        Ok(Self {
            handle,
            client,
            pinned_roots: trust_roots_pem.len(),
        })
    }

    /// Post `body` to `url` as JSON. Returns at once; the outcome arrives on the pending
    /// delivery.
    #[must_use]
    pub fn post_json(&self, url: &str, body: serde_json::Value) -> PendingDelivery {
        let (tx, rx) = mpsc::channel();
        let client = self.client.clone();
        let url = url.to_owned();
        self.handle.spawn(async move {
            let outcome = match client.post(&url).json(&body).send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if response.status().is_success() {
                        DeliveryOutcome::Accepted { status }
                    } else {
                        let body = response.text().await.unwrap_or_default();
                        DeliveryOutcome::Refused {
                            status,
                            body: body.chars().take(200).collect(),
                        }
                    }
                }
                Err(e) => DeliveryOutcome::Unreachable {
                    reason: e.to_string(),
                },
            };
            let _ = tx.send(outcome);
        });
        PendingDelivery { rx }
    }
}

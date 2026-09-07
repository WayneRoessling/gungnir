//! Authentication -- session establishment, credential verification. The concrete
//! mechanism (operator login on the desktop, mutual TLS or tokens for API callers)
//! is deployment-specific and not yet chosen; see ARCHITECTURE.md §8.5.

use crate::{OperatorId, SecurityError};

pub trait Authenticator: Send + Sync {
    fn authenticate(&self, credential: &[u8]) -> Result<OperatorId, SecurityError>;
}

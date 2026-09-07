//! The journal's durability policy (decision D-04, `ARCHITECTURE.md` §10 item 19).
//!
//! D-04 settles the fsync question **per deployment profile**, not once for the
//! workspace:
//!
//! - the **service node** journal fsyncs every envelope, because its budget is "an
//!   accepted envelope is on disk within 100 ms" (`docs/performance-budgets.md`) and
//!   it is the system of record for every connected desktop;
//! - **desktop** journals are buffered, with fsync on session save and every 5 s,
//!   because their budget is "under 1 ms for 50 envelopes" per frame and a desktop
//!   losing its last few seconds on a hard kill costs a local session, not the
//!   mission picture.
//!
//! [`DurabilityPolicy`] is that choice made explicit at the call site instead of
//! implied by which binary happens to be running.
//!
//! # What "fsync" means here
//!
//! `sync_data`, not `sync_all`: the file's contents reach the disk, but its metadata
//! (mtime, and on some filesystems the directory entry) may not. That is the right
//! trade for an append-only log whose length is discovered by reading it -- the data
//! is what has to survive, and `sync_data` is materially cheaper.
//!
//! Note that the previous implementation called `Write::flush` on a `File`, which for
//! `std::fs::File` does nothing at all: it is not a `fsync` and never was. The node
//! profile was therefore not meeting the durability D-04 asked of it either, which is
//! why the fix is a policy rather than only a buffer.

use std::time::Duration;

/// How hard the journal works to get an envelope onto the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityPolicy {
    /// Fsync after every envelope. The service-node profile (D-04).
    ///
    /// Slowest and strongest: when `append` returns, the envelope is on the disk.
    SyncEveryEnvelope,
    /// Buffer, and fsync when [`crate::EventJournal::sync`] is called or when
    /// `fsync_interval` has elapsed since the last fsync, whichever comes first. The
    /// desktop profile (D-04).
    ///
    /// A hard kill can lose envelopes written since the last fsync. That is the
    /// documented trade, not an oversight: [`DESKTOP_FSYNC_INTERVAL`] bounds it.
    Buffered { fsync_interval: Duration },
}

/// The interval D-04 fixes for desktop journals.
pub const DESKTOP_FSYNC_INTERVAL: Duration = Duration::from_secs(5);

impl DurabilityPolicy {
    /// The service-node profile: fsync every envelope.
    #[must_use]
    pub const fn node() -> Self {
        DurabilityPolicy::SyncEveryEnvelope
    }

    /// The desktop profile: buffered, fsync on session save and every 5 s.
    #[must_use]
    pub const fn desktop() -> Self {
        DurabilityPolicy::Buffered {
            fsync_interval: DESKTOP_FSYNC_INTERVAL,
        }
    }

    /// Whether an fsync is owed after every single append.
    #[must_use]
    pub const fn syncs_every_envelope(self) -> bool {
        matches!(self, DurabilityPolicy::SyncEveryEnvelope)
    }

    /// The longest an accepted envelope may sit unsynced, or `None` when every
    /// envelope is synced.
    #[must_use]
    pub const fn max_unsynced(self) -> Option<Duration> {
        match self {
            DurabilityPolicy::SyncEveryEnvelope => None,
            DurabilityPolicy::Buffered { fsync_interval } => Some(fsync_interval),
        }
    }
}

/// The safe default is the strong one. A profile that wants to lose data on a crash
/// has to say so; nothing becomes less durable because a call site forgot to choose.
impl Default for DurabilityPolicy {
    fn default() -> Self {
        DurabilityPolicy::SyncEveryEnvelope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_strong_policy() {
        assert_eq!(DurabilityPolicy::default(), DurabilityPolicy::node());
        assert!(DurabilityPolicy::default().syncs_every_envelope());
        assert_eq!(DurabilityPolicy::default().max_unsynced(), None);
    }

    #[test]
    fn desktop_policy_matches_d04() {
        assert_eq!(
            DurabilityPolicy::desktop().max_unsynced(),
            Some(Duration::from_secs(5))
        );
        assert!(!DurabilityPolicy::desktop().syncs_every_envelope());
    }
}

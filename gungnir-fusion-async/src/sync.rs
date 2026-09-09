// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The crate's cross-task primitives in one place, so that **one body of code** --
//! [`crate::ingest_with`], unchanged -- compiles against the real channel in every
//! ordinary build and against a `loom`-instrumented one under `--cfg loom`.
//!
//! This module exists for Gate 4 (`.github/workflows/loom.yml`,
//! `docs/agentic-workflow.md`, `docs/agentic-coding-standards.md` §5), and it is the
//! shim the gate's own comment asks for. Nothing in it changes what an ordinary build
//! does: outside the model checks every item below is a re-export of the type this crate
//! has always used, so `gungnir-tracking-service` sees exactly the same public signatures
//! on [`crate::ingest`] and [`crate::ingest_with`] as before.
//!
//! # Why the cfg is `all(test, loom)` and not `loom`
//!
//! `loom` is a **dev**-dependency, so it is linked only into test targets; a plain
//! `cargo build --lib` under `--cfg loom` would see `use loom::..` with no `loom` crate
//! to resolve. Gating on `all(test, loom)` -- which is what `tokio` does with its own
//! loom shim, for the same reason -- means the loom-instrumented channel exists exactly
//! where loom does: in `cargo test --lib` under `--cfg loom`, which is the invocation
//! the gate runs. Every other build, with or without the flag, gets `crossbeam-channel`.
//!
//! # Why a channel of our own under `cfg(loom)`, rather than `loom::sync::mpsc`
//!
//! Two reasons, and the first is disqualifying.
//!
//! **`loom::sync::mpsc` does not model disconnection.** Its `try_recv` returns `Empty`
//! or a message and *never* `Disconnected` (loom 0.7.2, `src/sync/mpsc.rs`: it checks
//! `is_empty()` and otherwise delegates to the blocking `recv`). [`crate::ingest_with`]
//! terminates on `TryRecvError::Disconnected` and `gungnir-tracking-service::poll` reads
//! it to decide the pipeline is gone, so over `loom::sync::mpsc` the real loop would
//! never terminate and the one property most worth checking -- that the end of a stream
//! is a flush and not a truncation -- could not be expressed at all.
//!
//! **`crossbeam-channel` is opaque to loom.** loom only sees operations performed
//! through its own `Mutex`/`Arc`/atomics; `crossbeam-channel` synchronises with `std`
//! atomics and its own parking, none of which loom instruments. Compiling the real
//! crossbeam channel under `--cfg loom` would therefore explore no orderings of it --
//! which is a smaller version of exactly the defect GAP-061 found.
//!
//! So under `cfg(loom)` this module provides an unbounded MPSC channel with
//! `crossbeam-channel`'s documented contract, built from `loom::sync` primitives. **Say
//! plainly what that does and does not buy.** The `ingest` loop, the pipeline it drives
//! and the consumer protocol in `loom_model` are the real code. The channel underneath
//! them is a model of crossbeam's contract, not crossbeam: a bug *inside*
//! `crossbeam-channel` is out of reach here, and no amount of green from this gate is
//! evidence about crossbeam's own implementation. What is in reach is every ordering of
//! the crate's own use of it, which is where this crate's concurrency actually lives.

#[cfg(not(all(test, loom)))]
pub use self::real::{unbounded, Receiver, Sender, TryRecvError};

#[cfg(all(test, loom))]
pub use self::modelled::{unbounded, Receiver, Sender, TryRecvError};

#[cfg(not(all(test, loom)))]
mod real {
    pub use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
}

/// How often the ingest loop re-polls its inbound channel while idle.
#[cfg(not(all(test, loom)))]
const IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// What [`crate::ingest_with`] does when its inbound channel is open and empty.
///
/// A real sleep in an ordinary build; under `cfg(loom)` a scheduling point, because
/// `tokio::time::sleep` needs a timer a loom execution does not have.
///
/// **No model in `loom_model` reaches this**, deliberately: each of them finishes
/// sending and drops the producer's sender before the ingest task is spawned, so
/// `try_recv` returns a message or `Disconnected` and never `Empty`. That is a real
/// limitation of the models and it is stated in `loom_model`'s own documentation --
/// a `try_recv`/yield spin against a channel that stays open is unbounded, and loom
/// explores schedules in which one thread runs indefinitely, so a model that reached
/// this path would exhaust `LOOM_MAX_BRANCHES` rather than report anything.
#[cfg(not(all(test, loom)))]
pub(crate) async fn idle_backoff() {
    tokio::time::sleep(IDLE_POLL).await;
}

// `async` with nothing to await, deliberately: the call site is `.await`ed inside
// `ingest_with` and that loop is not rewritten to suit the model checks. A loom
// scheduling point is the whole of what this needs to be.
#[cfg(all(test, loom))]
#[allow(clippy::unused_async)]
pub(crate) async fn idle_backoff() {
    loom::thread::yield_now();
}

/// An unbounded MPSC channel with `crossbeam-channel`'s contract, over `loom::sync`.
#[cfg(all(test, loom))]
mod modelled {
    use loom::sync::atomic::{AtomicUsize, Ordering};
    use loom::sync::{Arc, Mutex};
    use std::collections::VecDeque;

    use std::sync::mpsc::SendError;
    /// The same two cases `crossbeam_channel::TryRecvError` has, and the same meaning:
    /// `Empty` says "not yet", `Disconnected` says "not ever".
    pub use std::sync::mpsc::TryRecvError;
    use std::sync::PoisonError;

    struct Shared<T> {
        queue: Mutex<VecDeque<T>>,
        senders: AtomicUsize,
        receivers: AtomicUsize,
    }

    pub struct Sender<T> {
        shared: Arc<Shared<T>>,
    }

    pub struct Receiver<T> {
        shared: Arc<Shared<T>>,
    }

    pub fn unbounded<T>() -> (Sender<T>, Receiver<T>) {
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            senders: AtomicUsize::new(1),
            receivers: AtomicUsize::new(1),
        });
        (
            Sender {
                shared: Arc::clone(&shared),
            },
            Receiver { shared },
        )
    }

    impl<T> Sender<T> {
        /// `Err` once the receiver is gone, which is what [`crate::ingest_with`] reads
        /// to decide the track consumer has stopped listening.
        pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
            // The liveness read is inside the queue lock so that it is ordered against
            // the receiver's own drain, rather than being a separate racy peek.
            //
            // Poisoning is recovered from rather than unwrapped: a panic in one model
            // thread is already the failure loom is about to report, and turning it into
            // a second panic here would replace loom's execution trace -- the useful
            // half -- with a poison error. `unwrap()` is also not permitted in `src/`
            // (§3.1, and `gungnir-app/tests/architecture_compliance.rs` enforces it).
            let mut queue = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if self.shared.receivers.load(Ordering::Acquire) == 0 {
                return Err(SendError(msg));
            }
            queue.push_back(msg);
            Ok(())
        }
    }

    impl<T> Clone for Sender<T> {
        fn clone(&self) -> Self {
            self.shared.senders.fetch_add(1, Ordering::Relaxed);
            Sender {
                shared: Arc::clone(&self.shared),
            }
        }
    }

    impl<T> Drop for Sender<T> {
        fn drop(&mut self) {
            // Release, paired with the Acquire in `try_recv`: everything this sender did
            // before dropping must be visible to a receiver that observes the count at
            // zero, or "the channel is closed" would be a claim made ahead of the data.
            self.shared.senders.fetch_sub(1, Ordering::Release);
        }
    }

    impl<T> Receiver<T> {
        /// **The order of the two steps below is the whole contract**, and it is what
        /// `loom_model::flush_snapshot_survives_the_producers_disconnect` checks.
        ///
        /// The queue is drained *first* and the sender count is consulted only once the
        /// queue is empty, both under the same lock. Reading the count first would let a
        /// message that was sent before the last sender dropped be reported as a
        /// disconnect and lost -- which for this crate means the final flush snapshot,
        /// the one `ingest_with` emits precisely so that "the stream's end is a flush,
        /// not a truncation". Because the count can only reach zero after every send has
        /// completed, and because a sender cannot push while this lock is held, an empty
        /// queue observed together with a zero count means every message ever sent has
        /// already been taken.
        pub fn try_recv(&self) -> Result<T, TryRecvError> {
            let mut queue = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(msg) = queue.pop_front() {
                return Ok(msg);
            }
            if self.shared.senders.load(Ordering::Acquire) == 0 {
                Err(TryRecvError::Disconnected)
            } else {
                Err(TryRecvError::Empty)
            }
        }
    }

    impl<T> Drop for Receiver<T> {
        fn drop(&mut self) {
            self.shared.receivers.fetch_sub(1, Ordering::Release);
        }
    }
}

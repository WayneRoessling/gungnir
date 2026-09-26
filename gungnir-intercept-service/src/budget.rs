// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The solve budget and the clock it is measured on (GAP-119, D-81; DN-04 §10).
//!
//! **Why a budget.** The planner runs inside the tick: the desktop's `update()` and the
//! node's loop both call it in line, and the exact dynamic program in
//! `gungnir-allocation` is exponential in the track count, so a large enough picture
//! would hold the frame for as long as the solve takes. MOP-06
//! (`docs/mission/measures.md` §2) sets the planner's own target -- "p99 under the
//! per-frame budget of 4 ms for the embedded profile ... last-good-plan return beyond"
//! -- and this module is that clause made real. A planning call spends at most the
//! budget solving; a solve that has not finished by then is kept and carried on by the
//! next call (`gungnir_allocation::ExactSolve`), and meanwhile the planner answers with
//! the last plan it did compute, labelled stale and stamped with when
//! (`crate::PlanOutcome::Stale`).
//!
//! **Why carried on rather than moved to a thread.** MOP-06 says "off-thread" beyond the
//! budget, and the purpose of that is a frame that is never held; slicing the solve
//! across calls serves the same purpose without a second thread, a channel or a solve
//! whose result lands at a time nothing chose, and it keeps the overrun decidable in a
//! test. D-81 records the choice.
//!
//! **Why the clock is injected.** Whether a solve overran is a fact about elapsed time,
//! and a test of it that slept on the wall clock would pass or fail with the machine's
//! load -- on this workspace's hybrid development box, by a factor of two between core
//! classes. [`SolveClock`] is the one place time is read; [`MonotonicClock`] reads the
//! machine's monotonic clock and [`SteppedClock`] advances by a fixed step on every
//! reading, so a test states exactly which solve overruns.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The planner's budget when a deployment names none: MOP-06's 4 ms (D-81).
///
/// `gungnir-config`'s default for `plan_solve_budget_ms` is the same figure, and
/// `gungnir-app/tests/solve_budget.rs` fails if the two ever part.
pub const DEFAULT_SOLVE_BUDGET: Duration = Duration::from_millis(4);

/// A monotonic reading the planner measures a solve against.
///
/// Only the difference between two readings means anything; the origin is the
/// implementation's. `Send + Sync` because the planner behind an `InterceptService` is.
pub trait SolveClock: Send + Sync {
    /// Time since this clock's own origin. Never decreases.
    fn now(&self) -> Duration;
}

/// The machine's monotonic clock: what every deployed planner measures with.
#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl SolveClock for MonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// A clock that advances by a set step every time it is read, and otherwise stands
/// still.
///
/// **For making a solve overrun on purpose, deterministically.** With a step of zero a
/// solve takes no time at all and is always inside its budget; with a step larger than
/// the budget, the first question the solver asks finds the budget spent. The step can
/// be changed between two planning calls through a shared handle, which is how a test
/// has one solve succeed and the next overrun without sleeping.
#[derive(Debug, Default)]
pub struct SteppedClock {
    reading_ns: AtomicU64,
    step_ns: AtomicU64,
}

impl SteppedClock {
    #[must_use]
    pub fn new(step: Duration) -> Self {
        let clock = Self::default();
        clock.set_step(step);
        clock
    }

    /// The advance applied at every later reading.
    pub fn set_step(&self, step: Duration) {
        self.step_ns.store(
            u64::try_from(step.as_nanos()).unwrap_or(u64::MAX),
            Ordering::SeqCst,
        );
    }
}

impl SolveClock for SteppedClock {
    fn now(&self) -> Duration {
        let step = self.step_ns.load(Ordering::SeqCst);
        let before = self.reading_ns.fetch_add(step, Ordering::SeqCst);
        Duration::from_nanos(before.saturating_add(step))
    }
}

/// A budget in words, as a reason sentence and PN-05 print it: "4 ms", "0.5 ms".
#[must_use]
pub fn describe(budget: Duration) -> String {
    let ms = budget.as_secs_f64() * 1e3;
    if (ms - ms.round()).abs() < 1e-9 {
        format!("{ms:.0} ms")
    } else {
        format!("{ms} ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stepped_clock_advances_only_when_read() {
        let clock = SteppedClock::new(Duration::from_millis(5));
        assert_eq!(clock.now(), Duration::from_millis(5));
        assert_eq!(clock.now(), Duration::from_millis(10));
        clock.set_step(Duration::ZERO);
        assert_eq!(clock.now(), Duration::from_millis(10));
        assert_eq!(clock.now(), Duration::from_millis(10));
    }

    #[test]
    fn the_monotonic_clock_never_goes_back() {
        let clock = MonotonicClock::new();
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }

    #[test]
    fn a_budget_reads_as_a_person_would_write_it() {
        assert_eq!(describe(DEFAULT_SOLVE_BUDGET), "4 ms");
        assert_eq!(describe(Duration::from_micros(500)), "0.5 ms");
    }
}

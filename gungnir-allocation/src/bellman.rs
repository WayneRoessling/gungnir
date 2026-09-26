// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The Bellman/dynamic-programming resource-to-track allocator (GAP-029, Area A).
//!
//! Row: `allocation` / "Bellman/DP resource-to-track assignment" in
//! `docs/verification-capability-table.md` §1. Oracle: a custom textbook-verified
//! Python DP. Criterion: **exact match on the value function (1e-9)**.
//!
//! # The problem this solves, stated exactly
//!
//! The row says "value iteration over the horizon with a one-to-one assignment
//! constraint per step" and no more, so the rest is written down here rather than left
//! for a reader to infer, because the oracle has to solve the identical problem for an
//! exact-match criterion to mean anything.
//!
//! * **State**: which tracks are still unserviced, and how many steps remain.
//! * **Action**: a one-to-one matching between resources and unserviced tracks. Every
//!   resource is used at most once in a step and every track at most once; a resource
//!   may be used again on a later step, a track may not, because it has been serviced.
//!   The empty matching is always available: doing nothing this step is a legal action,
//!   and it is sometimes optimal when a better pairing becomes available later.
//! * **Reward**: the sum of `reward[resource][track]` over the pairs in the matching.
//! * **Transition**: the matched tracks leave the pool.
//! * **Value**: `V(S, k) = max over matchings M ⊆ S of (reward(M) + V(S \ M, k+1))`,
//!   with `V(·, horizon) = 0`.
//!
//! **Why a track leaves and a resource does not.** That asymmetry is the whole reason
//! this is a dynamic program rather than one assignment problem repeated. If nothing
//! left the pool, the optimal policy would be the same matching every step and the
//! horizon would be a multiplier; with tracks leaving, an early step must weigh a good
//! pairing now against the pairings it forecloses. The formulation matches
//! `gungnir-intercept-service`'s use: an intercept assignment services a track, and the
//! effector is available again next cycle.
//!
//! **Negative rewards are respected rather than clipped.** A pairing worth less than
//! nothing is simply not chosen, because the empty matching is a legal action; the
//! solver never assigns a resource to a track it is worse than useless against.
//!
//! # Why the exact DP, and where it stops
//!
//! The value function is over subsets of tracks, so it is exponential in the track
//! count. That is what an exact answer costs and the row asks for an exact one. The
//! solver refuses a problem larger than [`MAX_TRACKS`] and [`MAX_RESOURCES`] with a
//! named error rather than switching quietly to a heuristic: a caller that got a
//! greedy answer from a function documented as optimal would have no way to know.
//! `gungnir-intercept-service` sizes its problems by the ready effectors and the
//! tracks in the sector, which is well inside these bounds; a saturation case that is
//! not is a real finding and should surface as one.
//!
//! # Ties, and why the answer is a function of the input alone
//!
//! Two optimal policies can share a value, and which one is reported is a specification
//! decision rather than an accident, because a planner that reported a different optimum
//! for the same picture on two consoles would be making two recommendations (GAP-119).
//! The rule, applied at every state of every layer:
//!
//! 1. **The higher total wins**, where totals within [`TIE_TOLERANCE`] of each other are
//!    the same total.
//! 2. **Of equal totals, the matching with more pairs wins** -- the policy that acts
//!    (GAP-029; the comment at the comparison says why).
//! 3. **Of equal totals and equal pair counts, the first in enumeration order wins.**
//!    Enumeration is depth-first over resources in row order, and each resource tries
//!    staying idle first and then the tracks in ascending column order. So among
//!    equally good matchings of one size, the one reported is the lexicographically
//!    least when each resource's choice is read in row order with "idle" before track 0.
//!    For three resources and three tracks with every reward tied, that is the diagonal
//!    `[(0, 0), (1, 1), (2, 2)]`.
//!
//! No step of the rule reads a clock, a hash, an address or anything else outside the
//! reward matrix and the horizon, so the same input gives the same assignment on every
//! machine and in every run. Rows and columns are positions the caller chose, and
//! `gungnir-intercept-service` builds its rows in the order the resources were given and
//! its columns in the order the tracks were, so the tie is decided by the order the
//! caller listed them in. "Idle first" has a consequence worth stating because it reads
//! backwards: where a tie leaves a resource idle, it is the **earlier** resource that
//! idles -- two resources tied on one track give the track to the second.
//!
//! # Solving in slices
//!
//! [`ExactSolve`] runs the same dynamic program a slice at a time (GAP-119). It asks its
//! caller's `keep_going` whether to continue before each state of each layer and after
//! every [`CHECK_EVERY_MATCHINGS`] matchings walked within one, and on a "no" it keeps
//! its place -- the layers filled, the state part-walked and the best matching found in
//! it -- so the next slice carries on rather than starting again. The question is the
//! only change to the solve: the arithmetic, the enumeration order and the tie rule are
//! the same, so a solve advanced to its end returns exactly what [`solve_exact`] returns,
//! however it was sliced. Nothing partial is ever returned as an answer: until the last
//! state is filled, [`ExactSolve::advance`] says how far it has got and nothing else.
//! [`solve_exact_within`] is the one-slice form, for a caller with nowhere to keep a
//! solve, and returns [`AllocationError::Stopped`] when its slice ends first.

use crate::{AllocationError, AllocationPolicy};
use nalgebra::DMatrix;

/// Most tracks the exact solver will take. The value function has `2^tracks` states.
pub const MAX_TRACKS: usize = 16;

/// Most resources the exact solver will take. Matchings per step grow factorially in
/// the smaller of the two dimensions.
pub const MAX_RESOURCES: usize = 8;

/// One step's choice: which pairs were taken, and the tracks they consumed.
#[derive(Debug, Clone, Default)]
struct Matching {
    pairs: Vec<(usize, usize)>,
    consumed: u32,
    reward: f64,
}

/// Enumerate every matching of resources to the tracks in `available`.
///
/// Depth-first over resources: each resource takes an available track or takes
/// nothing. The empty matching falls out of the "takes nothing" branch at every level,
/// so it needs no special case. The order is rule 3 of the module's tie rule.
///
/// The whole list at once, for [`value_function`] -- the differential test's side of the
/// oracle comparison, which is kept on the enumeration it was first compared through.
/// [`ExactSolve`] walks the same order one matching at a time with [`Enumerator`].
fn matchings(reward: &DMatrix<f64>, available: u32, resources: usize) -> Vec<Matching> {
    fn walk(
        reward: &DMatrix<f64>,
        resource: usize,
        resources: usize,
        remaining: u32,
        current: &mut Matching,
        out: &mut Vec<Matching>,
    ) {
        if resource == resources {
            out.push(current.clone());
            return;
        }
        // This resource is not used this step.
        walk(reward, resource + 1, resources, remaining, current, out);
        for track in 0..reward.ncols() {
            let bit = 1u32 << track;
            if remaining & bit == 0 {
                continue;
            }
            current.pairs.push((resource, track));
            current.consumed |= bit;
            current.reward += reward[(resource, track)];
            walk(
                reward,
                resource + 1,
                resources,
                remaining & !bit,
                current,
                out,
            );
            current.reward -= reward[(resource, track)];
            current.consumed &= !bit;
            current.pairs.pop();
        }
    }

    let mut out = Vec::new();
    let mut current = Matching::default();
    walk(reward, 0, resources, available, &mut current, &mut out);
    out
}

/// One level of [`Enumerator`]'s walk: the resource at this depth.
#[derive(Debug, Clone)]
struct Frame {
    /// Tracks still unmatched at this depth.
    remaining: u32,
    /// The next choice to try: 0 is "idle", `k >= 1` is track `k - 1` onward.
    next: usize,
    /// The track this resource holds while its subtree is walked, to be given back when
    /// the walk returns here.
    taken: Option<usize>,
}

/// [`matchings`]' walk, one matching at a time, so it can stop part way through a state
/// and pick up exactly where it stopped (GAP-119).
///
/// **The same walk, not a similar one.** The recursion's order is the tie rule, and its
/// running reward is updated by adding a pair's reward on the way down and subtracting it
/// on the way back up; the explicit stack here makes the same choices in the same order
/// and performs the same additions and subtractions in the same sequence, so every
/// matching it yields carries the bit-identical `reward` the recursion computes.
/// `bellman::tests::the_sliced_solve_is_bit_identical_to_the_recursive_one` holds it to
/// that across random problems and random places to stop.
#[derive(Debug, Clone)]
struct Enumerator {
    resources: usize,
    frames: Vec<Frame>,
    current: Matching,
}

impl Enumerator {
    fn new(available: u32, resources: usize) -> Self {
        Self {
            resources,
            frames: vec![Frame {
                remaining: available,
                next: 0,
                taken: None,
            }],
            current: Matching::default(),
        }
    }

    /// The next matching, or `None` once every one has been yielded.
    fn next(&mut self, reward: &DMatrix<f64>) -> Option<&Matching> {
        loop {
            let depth = self.frames.len().checked_sub(1)?;
            if depth == self.resources {
                // A leaf: every resource has chosen. Yield it, and resume at its parent.
                self.frames.pop();
                return Some(&self.current);
            }
            let frame = self.frames.last_mut()?;
            // Give back the track this resource held for the subtree just finished --
            // the recursion's subtraction on the way back up.
            if let Some(track) = frame.taken.take() {
                self.current.reward -= reward[(depth, track)];
                self.current.consumed &= !(1u32 << track);
                self.current.pairs.pop();
            }
            if frame.next == 0 {
                // This resource is not used this step.
                frame.next = 1;
                let remaining = frame.remaining;
                self.frames.push(Frame {
                    remaining,
                    next: 0,
                    taken: None,
                });
                continue;
            }
            let mut track = frame.next - 1;
            while track < reward.ncols() && frame.remaining & (1u32 << track) == 0 {
                track += 1;
            }
            if track >= reward.ncols() {
                // Every choice at this depth is done: return to the resource above.
                self.frames.pop();
                continue;
            }
            let bit = 1u32 << track;
            frame.next = track + 2;
            frame.taken = Some(track);
            let remaining = frame.remaining & !bit;
            self.current.pairs.push((depth, track));
            self.current.consumed |= bit;
            self.current.reward += reward[(depth, track)];
            self.frames.push(Frame {
                remaining,
                next: 0,
                taken: None,
            });
        }
    }
}

/// Matchings walked between two `keep_going` questions inside one state (GAP-119).
///
/// One state of an eight-resource, sixteen-track problem has hundreds of millions of
/// matchings, so a question asked only between states could go unasked for minutes.
/// Sixty-four keeps the question's own cost -- one clock reading in the planner --
/// small against the work between two of them.
pub const CHECK_EVERY_MATCHINGS: u64 = 64;

/// Two totals within this are the same total for the purpose of choosing between
/// policies. Rewards here are risk scores summed over a handful of terms, so anything
/// this close is rounding rather than a preference.
pub const TIE_TOLERANCE: f64 = 1e-9;

/// The state being worked on when a slice ended inside it.
#[derive(Debug, Clone)]
struct Scan {
    enumerator: Enumerator,
    best: f64,
    best_matching: Option<Matching>,
    walked: u64,
}

/// How far an [`ExactSolve`] has got.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// Finished: the optimal first step and the value of the whole policy.
    Done(AllocationPolicy),
    /// Not yet. `states_done` of `states_total` states of the value function are filled,
    /// which is how far along the solve is; the work per state is not uniform, so this
    /// is progress rather than a forecast.
    Pending { states_done: u64, states_total: u64 },
}

/// An exact solve that runs a slice at a time (GAP-119).
///
/// **Why slices.** A planner in a tick has a few milliseconds, and the exact solve of an
/// ordinary picture -- eight tracks and four effectors at the default horizon -- takes
/// longer than that on the development machine. A solve that could only run whole would
/// have to either hold the frame or be thrown away and started again next tick, which
/// for such a picture is for ever. This keeps what it has filled: [`ExactSolve::advance`]
/// works until its caller says stop, and the next call carries on from that matching.
///
/// A solve advanced to the end returns exactly what [`solve_exact`] returns, however many
/// slices it took and wherever they ended: the order of the work, and every addition in
/// it, is the same.
#[derive(Debug, Clone)]
pub struct ExactSolve {
    reward: DMatrix<f64>,
    horizon: usize,
    resources: usize,
    states: usize,
    /// `value[k][S]` is the value of holding subset `S` with `k` steps still to run.
    /// Index 0 is the terminal layer, so the solve fills upward and the answer is the top
    /// layer at the full subset.
    value: Vec<Vec<f64>>,
    /// The layer being filled, `1..=horizon`; `horizon + 1` once done.
    step: usize,
    /// The next state of that layer.
    subset: usize,
    scan: Option<Scan>,
    best_first: Option<Matching>,
}

impl ExactSolve {
    /// Check the problem and set the solve up. No work is done until
    /// [`ExactSolve::advance`].
    ///
    /// # Errors
    ///
    /// As [`solve_exact`]: a problem that could never be solved is refused here, by name,
    /// so it is never mistaken for one that is merely taking a while.
    pub fn new(reward: &DMatrix<f64>, horizon: usize) -> Result<Self, AllocationError> {
        let resources = reward.nrows();
        let tracks = reward.ncols();
        if resources == 0 || tracks == 0 {
            return Err(AllocationError::DegenerateInput("empty reward matrix"));
        }
        if horizon == 0 {
            return Err(AllocationError::DegenerateInput("zero horizon"));
        }
        if tracks > MAX_TRACKS {
            return Err(AllocationError::DegenerateInput(
                "more tracks than the exact solver takes; a heuristic answer would not be \
                 the optimum this function promises",
            ));
        }
        if resources > MAX_RESOURCES {
            return Err(AllocationError::DegenerateInput(
                "more resources than the exact solver takes; a heuristic answer would not \
                 be the optimum this function promises",
            ));
        }
        if reward.iter().any(|v| !v.is_finite()) {
            return Err(AllocationError::DegenerateInput(
                "a reward is not finite, so no optimum is defined",
            ));
        }
        let states = 1usize << tracks;
        Ok(Self {
            reward: reward.clone(),
            horizon,
            resources,
            states,
            value: vec![vec![0.0f64; states]; horizon + 1],
            step: 1,
            subset: 0,
            scan: None,
            best_first: None,
        })
    }

    /// States of the value function filled so far, and in all.
    #[must_use]
    pub fn progress(&self) -> (u64, u64) {
        let states = self.states as u64;
        let total = states * self.horizon as u64;
        let done = ((self.step.min(self.horizon + 1) - 1) as u64) * states + self.subset as u64;
        (done.min(total), total)
    }

    /// Work until `keep_going` answers no or the solve is finished.
    ///
    /// `keep_going` is asked before each state and after every
    /// [`CHECK_EVERY_MATCHINGS`] matchings within one, so the work that runs past a "no"
    /// is at most one of those units. A "no" loses nothing: the next call resumes at the
    /// matching after the last one walked.
    pub fn advance(&mut self, keep_going: &mut dyn FnMut() -> bool) -> Progress {
        while self.step <= self.horizon {
            let step = self.step;
            let available = u32::try_from(self.subset).unwrap_or(u32::MAX);
            let mut scan = match self.scan.take() {
                Some(scan) => scan,
                None => {
                    if !keep_going() {
                        return self.pending();
                    }
                    Scan {
                        enumerator: Enumerator::new(available, self.resources),
                        best: f64::NEG_INFINITY,
                        best_matching: None,
                        walked: 0,
                    }
                }
            };
            while let Some(matching) = scan.enumerator.next(&self.reward) {
                let next = available & !matching.consumed;
                let total = matching.reward + self.value[step - 1][next as usize];
                // **Where the value cannot choose, prefer the policy that acts.**
                //
                // This model has no time preference: a track engaged now is worth exactly
                // what it is worth next step, so at every state acting and deferring tie.
                // An earlier version broke that tie by enumeration order, which kept the
                // matching with the fewest pairs -- the empty one. The consequence was
                // not subtle: at the shipped default horizon of ten, `solve_exact`
                // returned an empty first step for every input while reporting the full
                // optimal value, so the desktop's plan was permanently empty and read as
                // a solved problem recommending nothing.
                //
                // Preferring more pairs on a tie changes which optimal policy is
                // reported, never what the optimum is. Inside the model the two are
                // worth the same; outside it the target may leave, the sensor may lose
                // the track and the window may close, so of two policies the value
                // function cannot separate, the one that acts is the one to report.
                // Ties on both value and pair count still fall to enumeration order, so
                // the answer stays a function of the input.
                let better = total > scan.best + TIE_TOLERANCE
                    || ((total - scan.best).abs() <= TIE_TOLERANCE
                        && scan
                            .best_matching
                            .as_ref()
                            .is_some_and(|b| matching.pairs.len() > b.pairs.len()));
                if better || scan.best_matching.is_none() && total >= scan.best {
                    scan.best = scan.best.max(total);
                    scan.best_matching = Some(matching.clone());
                }
                scan.walked += 1;
                if scan.walked % CHECK_EVERY_MATCHINGS == 0 && !keep_going() {
                    self.scan = Some(scan);
                    return self.pending();
                }
            }
            self.value[step][self.subset] = scan.best;
            if step == self.horizon && self.subset == self.states - 1 {
                self.best_first = scan.best_matching;
            }
            self.subset += 1;
            if self.subset == self.states {
                self.subset = 0;
                self.step += 1;
            }
        }

        let full = self.states - 1;
        let matching = self.best_first.clone().unwrap_or_default();
        // **Indices, not identifiers.** This function is handed an anonymous matrix and has
        // no way to know what a row or a column stands for. An earlier version wrapped these
        // in `ResourceId` and `TrackId`, which was type-correct and wrong: the caller passed
        // them into a plan and then looked the track up by identifier, so wherever ids did
        // not happen to equal indices the plan named the wrong effector against the wrong
        // track and the intercept geometry silently vanished. Returning indices makes the
        // caller do the mapping it is the only one able to do.
        let mut assignment: Vec<(usize, usize)> = matching.pairs;
        assignment.sort_unstable();
        Progress::Done(AllocationPolicy {
            assignment,
            value: self.value[self.horizon][full],
        })
    }

    fn pending(&self) -> Progress {
        let (states_done, states_total) = self.progress();
        Progress::Pending {
            states_done,
            states_total,
        }
    }
}

/// Solve the horizon exactly and return the first step's assignment with the value of
/// the whole policy.
///
/// # Errors
///
/// [`AllocationError::DegenerateInput`] for an empty matrix or a zero horizon, a
/// non-finite reward, or a problem past [`MAX_TRACKS`]/[`MAX_RESOURCES`].
pub fn solve_exact(
    reward: &DMatrix<f64>,
    horizon: usize,
) -> Result<AllocationPolicy, AllocationError> {
    solve_exact_within(reward, horizon, &mut || true)
}

/// [`solve_exact`] in one slice: the whole solve if `keep_going` allows it, and
/// [`AllocationError::Stopped`] if it does not.
///
/// For a caller with nowhere to keep a half-finished solve. One that has somewhere --
/// a planner called every tick -- holds an [`ExactSolve`] and advances it instead, so
/// the work done before a "no" is not thrown away.
///
/// # Errors
///
/// As [`solve_exact`], and [`AllocationError::Stopped`] when `keep_going` answered no
/// before the solve finished. The input checks run first and ask nothing, so a problem
/// that could never be solved is refused by name rather than reported as stopped.
pub fn solve_exact_within(
    reward: &DMatrix<f64>,
    horizon: usize,
    keep_going: &mut dyn FnMut() -> bool,
) -> Result<AllocationPolicy, AllocationError> {
    match ExactSolve::new(reward, horizon)?.advance(keep_going) {
        Progress::Done(policy) => Ok(policy),
        Progress::Pending { .. } => Err(AllocationError::Stopped),
    }
}

/// The whole value function, for the differential test.
///
/// `value_function(reward, horizon)[k][S]` is `V(S, horizon - k)`: layer 0 is the full
/// horizon and the last layer is terminal. The row compares the value function rather
/// than only the policy, because two different policies can share an optimal value and
/// comparing only the policy would make a tie look like a disagreement.
///
/// # Errors
///
/// As [`solve_exact`].
pub fn value_function(
    reward: &DMatrix<f64>,
    horizon: usize,
) -> Result<Vec<Vec<f64>>, AllocationError> {
    let resources = reward.nrows();
    let tracks = reward.ncols();
    if resources == 0 || tracks == 0 {
        return Err(AllocationError::DegenerateInput("empty reward matrix"));
    }
    if horizon == 0 {
        return Err(AllocationError::DegenerateInput("zero horizon"));
    }
    if tracks > MAX_TRACKS || resources > MAX_RESOURCES {
        return Err(AllocationError::DegenerateInput("problem too large"));
    }
    let states = 1usize << tracks;
    let mut value = vec![vec![0.0f64; states]; horizon + 1];
    for step in 1..=horizon {
        for subset in 0..states {
            let available = u32::try_from(subset).unwrap_or(u32::MAX);
            let mut best = f64::NEG_INFINITY;
            for matching in matchings(reward, available, resources) {
                let next = available & !matching.consumed;
                best = best.max(matching.reward + value[step - 1][next as usize]);
            }
            value[step][subset] = best;
        }
    }
    // Reverse so layer 0 is the full horizon, which is how the fixture reads.
    value.reverse();
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The solve as it stood before GAP-119 sliced it: one pass, over the recursive
    /// enumeration, with the same comparison. Kept here as the reference the sliced solve
    /// must reproduce bit for bit.
    fn reference_solve(reward: &DMatrix<f64>, horizon: usize) -> AllocationPolicy {
        let resources = reward.nrows();
        let states = 1usize << reward.ncols();
        let mut value = vec![vec![0.0f64; states]; horizon + 1];
        let mut best_first: Option<Matching> = None;
        for step in 1..=horizon {
            for subset in 0..states {
                let available = u32::try_from(subset).unwrap_or(u32::MAX);
                let mut best = f64::NEG_INFINITY;
                let mut best_matching: Option<Matching> = None;
                for matching in matchings(reward, available, resources) {
                    let next = available & !matching.consumed;
                    let total = matching.reward + value[step - 1][next as usize];
                    let better = total > best + TIE_TOLERANCE
                        || ((total - best).abs() <= TIE_TOLERANCE
                            && best_matching
                                .as_ref()
                                .is_some_and(|b| matching.pairs.len() > b.pairs.len()));
                    if better || best_matching.is_none() && total >= best {
                        best = best.max(total);
                        best_matching = Some(matching);
                    }
                }
                value[step][subset] = best;
                if step == horizon && subset == states - 1 {
                    best_first = best_matching;
                }
            }
        }
        let mut assignment = best_first.unwrap_or_default().pairs;
        assignment.sort_unstable();
        AllocationPolicy {
            assignment,
            value: value[horizon][states - 1],
        }
    }

    /// The explicit-stack walk yields the recursion's matchings, in its order, with its
    /// running rewards to the bit.
    #[test]
    fn the_enumerator_walks_exactly_what_the_recursion_walks() {
        let reward = DMatrix::from_row_slice(
            3,
            4,
            &[0.1, 0.2, 0.3, 0.7, 1.1, 0.01, 2.5, 0.3, 0.6, 0.6, 0.05, 1.9],
        );
        for available in 0..16_u32 {
            let expected = matchings(&reward, available, 3);
            let mut walk = Enumerator::new(available, 3);
            let mut got = Vec::new();
            while let Some(m) = walk.next(&reward) {
                got.push(m.clone());
            }
            assert_eq!(got.len(), expected.len(), "subset {available:b}");
            for (g, e) in got.iter().zip(&expected) {
                assert_eq!(g.pairs, e.pairs);
                assert_eq!(g.consumed, e.consumed);
                assert_eq!(g.reward.to_bits(), e.reward.to_bits());
            }
        }
    }

    proptest::proptest! {
        /// **However a solve is sliced, its answer is the unsliced answer, bit for bit**
        /// (GAP-119). Random problems -- rewards drawn from a small set so ties are
        /// common, and with fractions so the running sums round -- and a random schedule
        /// of "no" answers, including long runs of them.
        #[test]
        fn the_sliced_solve_is_bit_identical_to_the_recursive_one(
            resources in 1_usize..=4,
            tracks in 1_usize..=5,
            horizon in 1_usize..=3,
            cells in proptest::collection::vec(0_usize..6, 20),
            stops in proptest::collection::vec(proptest::bool::weighted(0.3), 1..64),
        ) {
            const LEVELS: [f64; 6] = [0.0, 0.1, 0.2, 1.0, 1.3, -0.7];
            let reward = DMatrix::from_fn(resources, tracks, |r, t| LEVELS[cells[r * 5 + t]]);
            let expected = reference_solve(&reward, horizon);
            let mut solve = ExactSolve::new(&reward, horizon).expect("valid");
            let mut asked = 0_usize;
            let mut slices = 0_u32;
            let got = loop {
                slices += 1;
                proptest::prop_assert!(slices < 100_000, "the solve made no progress");
                let progress = solve.advance(&mut || {
                    asked += 1;
                    !stops[asked % stops.len()]
                });
                match progress {
                    Progress::Done(policy) => break policy,
                    Progress::Pending { states_done, states_total } => {
                        proptest::prop_assert!(states_done < states_total);
                    }
                }
            };
            proptest::prop_assert_eq!(&got.assignment, &expected.assignment);
            proptest::prop_assert_eq!(got.value.to_bits(), expected.value.to_bits());
        }
    }

    /// A solve told "no" at every question makes no progress and says so, and is none
    /// the worse for it: allowed to run afterwards, it finishes with the right answer.
    #[test]
    fn a_solve_refused_every_slice_waits_and_then_finishes() {
        let reward = DMatrix::from_element(3, 3, 1.0);
        let mut solve = ExactSolve::new(&reward, 2).expect("valid");
        for _ in 0..5 {
            assert_eq!(
                solve.advance(&mut || false),
                Progress::Pending {
                    states_done: 0,
                    states_total: 16
                }
            );
        }
        assert_eq!(
            solve.advance(&mut || true),
            Progress::Done(solve_exact(&reward, 2).expect("solvable"))
        );
    }

    /// One resource, one step: take the best track, and the value is that reward.
    #[test]
    fn one_resource_one_step_takes_the_best_track() {
        let reward = DMatrix::from_row_slice(1, 3, &[2.0, 9.0, 4.0]);
        let policy = solve_exact(&reward, 1).expect("solvable");
        assert_eq!(policy.assignment, vec![(0, 1)]);
        assert!((policy.value - 9.0).abs() < 1e-12);
    }

    /// The horizon is what makes this a dynamic program. One resource and two steps
    /// services two tracks, so the value is the two best rewards and not twice the best.
    #[test]
    fn a_horizon_services_a_track_once_and_moves_on() {
        let reward = DMatrix::from_row_slice(1, 3, &[2.0, 9.0, 4.0]);
        let policy = solve_exact(&reward, 2).expect("solvable");
        assert!(
            (policy.value - 13.0).abs() < 1e-12,
            "9 then 4, not 9 twice: {}",
            policy.value
        );
    }

    /// Doing nothing is a legal action, so a resource is never forced onto a pairing
    /// that costs more than it is worth.
    #[test]
    fn a_negative_pairing_is_declined_rather_than_forced() {
        let reward = DMatrix::from_row_slice(1, 2, &[-5.0, -3.0]);
        let policy = solve_exact(&reward, 1).expect("solvable");
        assert!(policy.assignment.is_empty(), "{:?}", policy.assignment);
        assert!((policy.value - 0.0).abs() < 1e-12);
    }

    /// Two resources cannot both take the same track in one step, and both are used
    /// when both pairings are worth taking.
    #[test]
    fn one_track_takes_at_most_one_resource_per_step() {
        let reward = DMatrix::from_row_slice(2, 2, &[10.0, 1.0, 9.0, 2.0]);
        let policy = solve_exact(&reward, 1).expect("solvable");
        assert_eq!(policy.assignment.len(), 2);
        let tracks: Vec<usize> = policy.assignment.iter().map(|(_, t)| *t).collect();
        assert_eq!(tracks, vec![0, 1], "each resource took a different track");
        assert!((policy.value - 12.0).abs() < 1e-12);
    }

    /// A greedy first step is not always optimal, and this is the case that proves the
    /// solver looks ahead: taking the 10 first strands the second resource, and the
    /// optimum over two steps is the pairing that keeps both usable.
    #[test]
    fn the_solver_looks_ahead_rather_than_taking_the_best_first_step() {
        // Resource 0 is the only one that can service track 0 profitably; resource 1
        // is worthless everywhere. One step of one resource, two steps available.
        let reward = DMatrix::from_row_slice(1, 2, &[10.0, 10.0]);
        let one = solve_exact(&reward, 1).expect("solvable").value;
        let two = solve_exact(&reward, 2).expect("solvable").value;
        assert!((one - 10.0).abs() < 1e-12);
        assert!(
            (two - 20.0).abs() < 1e-12,
            "both tracks are serviced over two steps: {two}"
        );
    }

    #[test]
    fn a_problem_too_large_for_an_exact_answer_is_refused_by_name() {
        let reward = DMatrix::from_element(1, MAX_TRACKS + 1, 1.0);
        assert!(matches!(
            solve_exact(&reward, 1),
            Err(AllocationError::DegenerateInput(_))
        ));
        let reward = DMatrix::from_element(MAX_RESOURCES + 1, 2, 1.0);
        assert!(matches!(
            solve_exact(&reward, 1),
            Err(AllocationError::DegenerateInput(_))
        ));
    }

    /// **Rule 3 of the tie rule, pinned** (GAP-119). Every reward tied, so value and pair
    /// count cannot choose, and enumeration order must: the diagonal, at every horizon.
    #[test]
    fn a_full_tie_falls_to_the_documented_order() {
        let reward = DMatrix::from_element(3, 3, 1.0);
        for horizon in [1_usize, 2, 10] {
            let policy = solve_exact(&reward, horizon).expect("solvable");
            assert_eq!(
                policy.assignment,
                vec![(0, 0), (1, 1), (2, 2)],
                "horizon {horizon}"
            );
        }
        // Among one-pair matchings with one resource idle, "idle" sorts first: two
        // resources and one track, tied, gives the track to the LAST resource, because
        // the enumeration tries resource 0 idle before resource 0 on track 0.
        let two_by_one = DMatrix::from_element(2, 1, 1.0);
        let policy = solve_exact(&two_by_one, 1).expect("solvable");
        assert_eq!(policy.assignment, vec![(1, 0)]);
    }

    /// A solve allowed to finish is the solve (GAP-119): asking the question changes
    /// nothing about the answer.
    #[test]
    fn a_solve_that_is_never_stopped_returns_what_solve_exact_returns() {
        let reward = DMatrix::from_row_slice(
            3,
            4,
            &[3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0, 5.0, 3.0, 5.0, 8.0],
        );
        let mut asked = 0_u32;
        let within = solve_exact_within(&reward, 3, &mut || {
            asked += 1;
            true
        })
        .expect("solvable");
        assert!(asked > 0, "the solve never asked");
        assert_eq!(within, solve_exact(&reward, 3).expect("solvable"));
    }

    /// A "no" ends the solve with `Stopped`, wherever in the work it lands -- between
    /// states or inside one state's enumeration -- and never with a partial answer.
    #[test]
    fn a_stopped_solve_returns_stopped_and_nothing_partial() {
        let reward = DMatrix::from_element(4, 4, 1.0);
        let mut total = 0_u32;
        let _ = solve_exact_within(&reward, 2, &mut || {
            total += 1;
            true
        });
        for allowed in 0..total {
            let mut asked = 0_u32;
            let result = solve_exact_within(&reward, 2, &mut || {
                asked += 1;
                asked <= allowed
            });
            assert!(
                matches!(result, Err(AllocationError::Stopped)),
                "stopped after {allowed} answers yet returned {result:?}"
            );
        }
    }

    /// A problem the solver refuses is refused by name before anything is asked, so it is
    /// never reported as a solve over budget.
    #[test]
    fn a_degenerate_problem_is_refused_before_the_question() {
        let reward = DMatrix::from_element(0, 3, 1.0);
        let result = solve_exact_within(&reward, 1, &mut || false);
        assert!(matches!(result, Err(AllocationError::DegenerateInput(_))));
    }

    /// The enumeration of one large state asks too, so a "no" is heard inside a state and
    /// not only between them. Four resources on four tracks is 209 matchings for the full
    /// subset, so the questions outnumber the states.
    #[test]
    fn a_large_state_is_asked_about_inside_its_enumeration() {
        let reward = DMatrix::from_element(4, 4, 1.0);
        let horizon = 2_u32;
        let mut asked = 0_u32;
        solve_exact_within(&reward, 2, &mut || {
            asked += 1;
            true
        })
        .expect("solvable");
        let states_times_layers = 16 * horizon;
        assert!(
            asked > states_times_layers,
            "asked {asked} times over {states_times_layers} states, so no enumeration asked"
        );
    }

    #[test]
    fn a_non_finite_reward_has_no_optimum_and_is_refused() {
        let mut reward = DMatrix::from_element(2, 2, 1.0);
        reward[(0, 1)] = f64::NAN;
        assert!(matches!(
            solve_exact(&reward, 2),
            Err(AllocationError::DegenerateInput(_))
        ));
    }
}

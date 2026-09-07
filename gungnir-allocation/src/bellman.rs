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
/// so it needs no special case.
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

/// Solve the horizon exactly and return the first step's assignment with the value of
/// the whole policy.
///
/// # Errors
///
/// [`AllocationError::DegenerateInput`] for an empty matrix or a zero horizon, a
/// non-finite reward, or a problem past [`MAX_TRACKS`]/[`MAX_RESOURCES`].
/// Two totals within this are the same total for the purpose of choosing between
/// policies. Rewards here are risk scores summed over a handful of terms, so anything
/// this close is rounding rather than a preference.
const TIE_TOLERANCE: f64 = 1e-9;

pub fn solve_exact(
    reward: &DMatrix<f64>,
    horizon: usize,
) -> Result<AllocationPolicy, AllocationError> {
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
    // `value[k][S]` is the value of holding subset `S` with `k` steps still to run.
    // Index 0 is the terminal layer, so the loop below fills upward and the answer is
    // the top layer at the full subset.
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

    let full = states - 1;
    let matching = best_first.unwrap_or_default();
    // **Indices, not identifiers.** This function is handed an anonymous matrix and has
    // no way to know what a row or a column stands for. An earlier version wrapped these
    // in `ResourceId` and `TrackId`, which was type-correct and wrong: the caller passed
    // them into a plan and then looked the track up by identifier, so wherever ids did
    // not happen to equal indices the plan named the wrong effector against the wrong
    // track and the intercept geometry silently vanished. Returning indices makes the
    // caller do the mapping it is the only one able to do.
    let mut assignment: Vec<(usize, usize)> = matching.pairs.clone();
    assignment.sort_unstable();
    Ok(AllocationPolicy {
        assignment,
        value: value[horizon][full],
    })
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

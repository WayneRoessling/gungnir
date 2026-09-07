# Five Scenarios, One Generator: Why This Set Covers the Matrix

Companion to `verification-capability-table.md` §1 (the matrix these scenarios cover) and `gungnir-capabilities.md` §1. The generator lives in `gungnir-scenario`, whose `Scenario` enum has one variant per scenario below; `gungnir-mission` is what turns a generated or recorded scenario into an operator-facing session, and `gungnir-replay` is what plays a *recorded* session back (the round-trip fidelity row here covers only synthetic, generated output). Scenario numbers are cited from Rust doc comments and must not change.

## The problem these five are solving

Look at the verification table and count how many rows list **"Scenario-crate generated"** (or a variant of it) as the data source: EKF, UKF, particle filter, IMM, the sqrt/UDU soak component, RTS smoothing, JPDA, MHT, track-manager lifecycle, PHD/CPHD, GLMB/LMB, the `fusion-async` OOS/multi-rate row, and both cross-cutting stability and performance rows. That's roughly half the table. If each of those got its own bespoke generator — a JPDA-shaped generator, an IMM-shaped generator, a PHD-shaped generator — the scenario crate would balloon into a pile of one-off fixtures that are individually easy to reason about but collectively impossible to maintain, and worse, would never catch the interaction bugs that only show up when a filter's output feeds an associator, which feeds a track manager, which feeds a fusion node.

The five scenarios below aren't arbitrary "realistic use cases" bolted on for flavor. Each one was chosen because it's the *cheapest single scenario* that forces multiple pipeline stages to run against each other honestly, rather than against a friendly, isolated stub. Read together, they form a small set of generator configurations — target-motion profile, sensor model, clutter/Pd model, multi-sensor timing, and geometry — that can be dialed up or down to produce every scenario-crate row in the table, plus a few of the "self-generated" and "public benchmark" rows as a bonus, without inventing new machinery for each one.

## Coverage map

| Scenario | Primary rows exercised | Secondary / incidental coverage |
|---|---|---|
| 1. Maneuvering aircraft | `core` CV/CA/CT (context), EKF, UKF, IMM | Establishes the single-target baseline the other four build on |
| 2. Maritime clutter | GNN, Hungarian, gating, JPDA, MHT, track-manager | `scenario` crate's own Pd/clutter statistical self-check |
| 3. Urban multi-sensor convoy | `fusion-async` OOS/multi-rate, `loom` concurrency, track-fusion CI, sensor registration/bias | Exercises the async pipeline under the same conditions the concurrency tests need |
| 4. Dense swarm/debris | PHD/CPHD, GLMB/LMB, particle filter | `metrics` (MOTA/MOTP, purity/fragmentation) at high target count |
| 5. Adversarial geometry + soak | `coord` pole/antimeridian, sqrt/UDU PSD stability, 10⁵-cycle stability soak, RTS/fixed-lag smoothing | Long-duration numerical drift detection |

Everything in that map traces back to a specific row and pass criterion in the table — nothing here is coverage for its own sake.

## Scenario 1 — Single maneuvering aircraft: the baseline everything else stands on

This scenario is deliberately the simplest: one target, one sensor, a clean CV → CT → CA motion sequence, nonlinear range/bearing/elevation measurements at a fixed rate. Its job isn't breadth, it's establishing ground truth for the nonlinear filters before anything else complicates the picture. EKF and UKF both need a scenario where the nonlinearity of the measurement model (spherical radar coordinates against Cartesian state) is real but not adversarial, so that a relative-error failure means the filter math is wrong, not that the scenario is degenerate. The coordinated turn segment is what makes IMM meaningful — a single mode never has to switch, so the IMM row's actual pass criterion (mode probabilities converging within 1e-3 during the CV↔CT transition) only gets exercised if the trajectory actually maneuvers.

This is also the scenario every other one implicitly reuses. Scenarios 2 through 5 all put multiple instances of "an object following one of these three motion models" into more hostile environments; none of them need to re-derive what a correct CV, CA, or CT trajectory looks like, because scenario 1 already validated that against the `core` module's closed-form matrices at the synthetic-fixture level. Scenario 1 is where you'd catch a broken Jacobian before it gets buried under clutter and association noise in scenario 2.

## Scenario 2 — Maritime clutter: where association and lifecycle earn their keep

Association algorithms are cheap to unit-test on a cost matrix and expensive to trust until they're run against a scenario with real ambiguity. Multiple vessels at different speeds, a configurable false-alarm rate, a configurable Pd, and a target that disappears behind a land mask together produce exactly the conditions GNN, the Hungarian/JV solver, and gating need: a cost matrix that isn't hand-picked to have an obvious answer. The land-mask dropout is what makes track-manager's coast/delete logic meaningful — without a real missed-detection run, "coast" is just a state that's never entered.

JPDA and MHT graduate naturally out of the same scenario once clutter density goes up enough that greedy nearest-neighbor starts producing wrong associations. Because this is the same underlying generator as the association tests (same sensor model, same clutter model, just tuned to a harder operating point), you get JPDA/MHT validation without inventing a second clutter model that has to be independently trusted.

The scenario-crate's own self-check — that empirical Pd and clutter rate converge to the configured values within 2σ — rides along for free here, because this is the first scenario where Pd and clutter rate are actually being *configured* rather than left at defaults. If the generator itself is subtly biased (e.g., clutter isn't uniform when it's supposed to be), this is where that surfaces, before it can quietly corrupt the JPDA and MHT results that depend on it.

## Scenario 3 — Urban convoy: the one scenario built to break timing

Every other scenario in this set can, in principle, be run through a single-rate, single-sensor, synchronous pipeline and still exercise its target rows. This one specifically cannot — it exists because `fusion-async`'s two rows (out-of-sequence handling / multi-rate fusion, and concurrency correctness) need a workload where sensors genuinely disagree about time. A slow camera, a faster radar, and a lidar with its own latency and occasional out-of-order arrivals is the minimum ingredient list for "out-of-sequence" to mean something real rather than a single artificially delayed message inserted into an otherwise clean stream.

Running `loom`'s exhaustive interleaving check against this same scenario's pipeline, rather than against a synthetic stress harness, means the concurrency test is validating the code path that real multi-rate fusion actually takes, not a simplified stand-in for it. That closes a real gap: it's possible to pass a `loom` test against an artificial harness and still have a race that only appears when the OOS buffer and the multi-rate scheduler interact under a specific timing pattern — which is precisely what this scenario is built to produce.

The second sensor platform with an unknown position/orientation bias does double duty. It feeds track-to-track CI/information-matrix fusion (multiple local tracks of the same convoy vehicles, fused into one global estimate) and it feeds sensor registration/bias estimation, where the pass criterion is recovering the injected bias within 1e-3. Because the bias is injected by construction into the same scenario that's already producing multi-sensor tracks, there's no need for a separate bias-only fixture — the registration check is just a different read of data the fusion test already needed.

## Scenario 4 — Dense swarm: the scenario that forces cardinality to matter

PHD/CPHD and GLMB/LMB are the two rows in the table where the pass criterion isn't "does the state match" but "does the *number* of things match, and does each thing's identity persist." Neither of those questions is well-posed with a handful of well-separated targets — a naive tracker gets cardinality right by accident when there's nothing to confuse it with. Tens to hundreds of closely-spaced objects with births, deaths, and near-simultaneous crossings is the minimum complexity at which cardinality estimation is actually being tested rather than assumed.

The particle filter earns its place here for a different reason: it's the one filter in the table whose oracle comparison is explicitly statistical (KS-test / mean-variance within 2σ) rather than exact, because it's meant for cases where EKF and UKF's Gaussian assumption breaks down. A bearings-only or narrow-FOV sensor on a subset of the swarm targets is what manufactures that condition — without a genuinely non-Gaussian, multimodal posterior, the particle filter would just be a noisier, slower way of computing the same answer EKF/UKF already gives, and the test wouldn't be checking anything a cheaper filter couldn't also validate.

Running `metrics` (MOTA/MOTP, purity, fragmentation) against this same high-density, high-ambiguity output rather than against a low-target-count scenario is intentional: those metrics are specifically sensitive to identity-switch and fragmentation errors, which are rare-to-nonexistent at low density. This scenario is the one place in the plan where the tracker output is actually stressed enough for a fragmentation count to be a meaningful number rather than a zero.

## Scenario 5 — Adversarial geometry and soak: the scenario built to find what the others can't

The first four scenarios are all, in their own way, "realistic." This one isn't trying to be — it exists to manufacture the specific numerical failure modes that realistic scenarios tend to avoid by construction. Antimeridian and near-pole crossings are where `coord`'s geodetic/ECEF/ENU/NED round-trip logic has historically broken in other tracking libraries (wraparound and singularity handling), and no amount of realistic maritime or aircraft data will reliably visit those coordinates unless the scenario deliberately routes a target through them.

Near-singular sensor-target-sensor geometry is the equivalent stress case for the square-root/UDU-factorized filters: their entire reason for existing is maintaining positive-semidefinite covariance under ill-conditioning that would push a standard-form filter into numerical trouble. A well-conditioned scenario — which is what scenarios 1 through 4 mostly are, since realism tends to avoid degeneracy — would never exercise the code path the sqrt/UDU form is actually there to protect. This scenario is where the "zero PSD violations over 10⁵+ cycles" pass criterion is actually meaningful, because the covariance is being pushed toward the edge on purpose.

The multi-day continuous run addresses the two remaining rows that need duration rather than difficulty: the numerical-stability soak test (drift and PSD violations that only appear after sustained operation, not in a scenario that ends after a few hundred steps) and RTS/fixed-lag smoothing, which needs a long, coherent trajectory to have anything meaningful to smooth over. A short trajectory can validate that the smoother's math is correct; only a long one can show whether the smoothed estimate stays coherent and whether the smoother itself accumulates error over an extended run.

## What this set deliberately leaves out — and why that's fine

A few rows in the table are conspicuously untouched by all five scenarios, and that's by design rather than oversight:

- **`core` CV/CA/CT, the linear KF, NN/Hungarian/gating, and allocation** all have exact-match pass criteria against closed-form references (matrix element comparison, exact cost/assignment, exact value function). These are correctness checks against a known-good computation, not behavioral checks against a realistic workload — a synthetic fixture is not just adequate for them, it's the *right* tool, because introducing scenario-crate realism would only add noise to a check that's supposed to be deterministic.
- **The two `scenario`-crate self-referential rows** (statistical self-check and round-trip fidelity) are validating the generator itself, not the trackers consuming it. Scenario 2 incidentally exercises the statistical self-check as a side effect, but round-trip fidelity (generate → export → replay → compare) is a property of the export/import code path and doesn't need any of these five narratives to be meaningful — it would pass or fail identically on a trivial single-target scenario.
- **Cross-cutting end-to-end accuracy and the schema round-trip row** are explicitly scoped to public benchmarks (MOT16/17/20, KITTI) and a corpus of recorded scenarios, not scenario-crate output — mixing them into this set would blur the line between "does our synthetic generator produce sound test cases" and "does the whole pipeline perform acceptably against externally validated data," which the table already keeps separate on purpose.

Leaving those out isn't a gap in the five-scenario plan; it's the plan correctly recognizing that a scenario generator is the right tool for behavioral and statistical validation, and the wrong tool for exact-match unit checks or external benchmark comparison.

## Suggested build order

Because scenario 1 is the dependency baseline and scenario 5 is the hardest to get numerically right, a practical build sequence is: **1 → 2 → 4 → 3 → 5**. Scenario 3's async/timing machinery is largely orthogonal to the others and can be built in parallel once the core generator (from scenario 1) exists, but validating it last means the track-to-track fusion inputs it needs have already been shaken out by scenarios 1 and 2. Scenario 5 should come last regardless of team bandwidth, since it's the one most likely to surface bugs in the underlying generator itself (coordinate handling, long-run stability) that would otherwise masquerade as filter bugs in the earlier scenarios.

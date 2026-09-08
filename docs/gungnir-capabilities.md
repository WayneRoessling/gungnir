# Gungnir Workspace — Capability Descriptions (Business Analyst View)

This document is the single, self-contained business-analyst capability reference
for the `gungnir-workspace` Cargo workspace: what each capability does, why the
business cares, how it is proven correct, what "done" looks like, and what breaks if
it is wrong. It replaces two earlier documents, `capability-descriptions.md` (the
tracking core) and `fusion-capability-descriptions.md` (everything else), which were
deleted on 2026-09-04; Rust doc comments and `Cargo.toml` descriptions that cited them
now cite the corresponding section here. Every crate name is the real `gungnir-`
name; the `fusion-` prefix the earlier draft used is retired. `ARCHITECTURE.md` is the
technical crate-boundary reference these business descriptions map onto, and
`ARCHITECTURE.md` §8 describes how the same crates deploy as a disconnected desktop,
an on-prem node, or a cloud node within the larger system of systems Gungnir belongs
to.

Section numbers §5.1–§5.6, §7, §8, and §9 are cited from Rust doc comments and must
not be renumbered.

**A note on implementation status, read this before anything else below:**
`gungnir-workspace` as it exists today is an **architectural scaffold**. Every crate
described in this document — including the original tracking core — has a real
`Cargo.toml`, correct dependency wiring, and doc-commented trait/type signatures, but
function bodies are `todo!()`. The difference between the tracking core (Part 1) and
everything else (Parts 2–5) is not "built vs. not built" — it's that the tracking
core has a mature, detailed, pre-existing verification specification
(`verification-capability-table.md`) defining exactly what "done" looks like once
implemented, while the newer crates (Parts 2–5) mostly don't have an equivalent
verification row yet. That gap is called out explicitly where it applies.

## How this document is organized

1. **Tracking & Estimation Core** — the original, verification-table-driven
   capabilities (`gungnir-core` through `gungnir-metrics`).
2. **Service Layer** — `gungnir-tracking-service`, `gungnir-intercept-service`.
3. **3D Data Ecosystem** — `gungnir-data`, `gungnir-data-fusion`.
4. **UI / Application** — `gungnir-render`, `gungnir-viewport3d`, `gungnir-ui`,
   `gungnir-app`.
5. **Productization Layer** — the twenty-one crates added to close the gaps an
   independent architecture review identified, plus the five added on 2026-09-04
   when the review's remaining recommendations were scaffolded, each with its
   implementation status.
6. **Independent assessment** — is the fusion sound?
7. **Recommended build increments** — a practical roadmap.
8. **Full crate map** — one table, all 50 crates, including the two deployment
   crates (`gungnir-remote`, `gungnir-node`).
9. **Principal risks** if the remaining gaps aren't addressed.

---

## 1. Tracking & Estimation Core

*Crates: `gungnir-core`, `gungnir-filters`, `gungnir-association`, `gungnir-track`,
`gungnir-rfs`, `gungnir-fusion-async`, `gungnir-track-fusion`, `gungnir-coord`,
`gungnir-allocation`, `gungnir-scenario`, `gungnir-metrics`, plus the cross-cutting
verification crates `gungnir-oracle`, `gungnir-testkit`, `gungnir-fuzz`.*

This is the mathematical foundation of the entire product. Every capability
described later in this document — the UI, the intercept planner, the ingestion
pipeline — ultimately consumes what this layer produces. Each entry below states
what it does, why the business cares, how it's verified, what "done" means, what
data validates it, and what breaks if it's wrong — the pass criteria are drawn from
`verification-capability-table.md` and remain the target this layer is built against.

### `gungnir-core` — Motion Models

**Motion Models: CV, CA, CT**

*What it does:* Provides the constant-velocity, constant-acceleration, and
coordinated-turn kinematic models that describe how a tracked object is expected to
move between sensor updates. These models generate the state-transition (`F`) and
process-noise (`Q`) matrices every downstream filter depends on.

*Why it matters:* This is the mathematical foundation of the entire tracking stack —
every filter, smoother, and fusion algorithm in the product ultimately predicts an
object's next position using one of these three models. An error here doesn't stay
contained; it silently propagates into every capability built on top of it.

*How it's verified:* Element-wise comparison of the generated `F`/`Q` matrices
against `filterpy.kalman` and hand-derived references in Python, and against
MATLAB's `initcvkf`/`initcakf`/`initctekf` functions, for the same initial state and
time step.

*Definition of done:* Matrix elements match to within ~1e-10 — effectively exact,
since this is a closed-form calculation with a single correct answer.

*Data used:* Synthetic fixtures only.

*Risk if wrong:* A broken Jacobian or transition matrix here would masquerade as a
filter, association, or fusion bug several layers downstream, and could go
undetected until scenario-based testing much later — which is why this is validated
first, before anything else.

### `gungnir-filters` — State Estimation

**Linear Kalman Filter**

*What it does:* Implements the standard (linear) Kalman filter — the baseline
recursive estimator for tracking a single object under linear motion and
measurement assumptions.

*Why it matters:* This is the reference implementation every more sophisticated
filter (EKF, UKF, IMM) is ultimately compared against or built out from, and the
correct default when linearity assumptions actually hold.

*How it's verified:* Same measurement sequence through the Rust implementation,
`filterpy.kalman.KalmanFilter`, and MATLAB's `trackingKF`; compare full state and
covariance trajectory, not just the final value.

*Definition of done:* State trajectory error under 1e-6; covariance Frobenius-norm
difference under 1e-6.

*Data used:* Synthetic fixture — a controlled, linear measurement sequence with a
known correct answer.

*Risk if wrong:* Since every other filter either extends or is validated against
this baseline, an undetected error here undermines confidence in the entire filter
suite.

**Extended Kalman Filter (EKF)**

*What it does:* Extends the Kalman filter to nonlinear measurement models (e.g.,
radar returning range/bearing/elevation against a Cartesian state) by linearizing
around the current estimate using the Jacobian.

*Why it matters:* Most real sensors (radar, sonar, camera) are nonlinear, so this is
the workhorse filter for realistic single-target tracking.

*How it's verified:* Same nonlinear scenario and Jacobians through the Rust EKF,
`filterpy.kalman.ExtendedKalmanFilter`, and MATLAB's `trackingEKF`; trajectories
compared.

*Definition of done:* Relative error under 1e-4.

*Data used:* Scenario-crate generated — the single-maneuvering-aircraft scenario.

*Risk if wrong:* Since the EKF is the most commonly deployed filter, an undetected
error has the broadest real-world blast radius of any single filter bug.

**Unscented Kalman Filter (UKF)**

*What it does:* Avoids explicit Jacobian computation by propagating a small set of
deterministically chosen sigma points through the nonlinear model.

*Why it matters:* More robust than EKF for strong nonlinearity or where Jacobians
are expensive/error-prone to derive.

*How it's verified:* Same sigma-point parameters through the Rust UKF, `filterpy`,
and MATLAB's `trackingUKF`; state and covariance compared.

*Definition of done:* Relative error under 1e-4; sigma-point weights sum to exactly
1 (structural check).

*Data used:* Scenario-crate generated (same maneuvering-aircraft scenario as EKF).

*Risk if wrong:* A subtle sigma-point weighting error could produce plausible but
biased estimates that only surface as accumulated drift over time.

**Particle Filter**

*What it does:* A sequential Monte Carlo estimator representing belief state as a
weighted particle set rather than a Gaussian, tracking genuinely non-Gaussian,
multimodal distributions.

*Why it matters:* The fallback when EKF/UKF's Gaussian assumption breaks down (e.g.,
ambiguous bearings-only tracking) — without it the product has no answer for that
class of problem.

*How it's verified:* Statistical comparison across many trials via KS-test or
mean/variance vs. `filterpy.monte_carlo`. No MATLAB oracle exists for this row.

*Definition of done:* KS-test passes, or mean/variance within 2σ of reference.

*Data used:* Scenario-crate generated, dense-swarm scenario's bearings-only sub-case
— deliberately engineered for a genuinely multimodal posterior.

*Risk if wrong:* Particle degeneracy/weight collapse is a classic silent failure —
the filter can look like it's working while having lost track of the true belief.

**Interacting Multiple Model (IMM)**

*What it does:* Runs several motion models (e.g., CV and CT) in parallel and blends
estimates using mode probabilities, so the tracker represents both "flying straight"
and "turning" without committing to one model in advance.

*Why it matters:* Real targets maneuver. IMM is the product's answer to "track
objects that behave like real objects."

*How it's verified:* Same model set/mode-transition matrix through the Rust IMM,
Stone Soup's `IMM`, and MATLAB's `trackingIMM`; mixed state estimate and mode
probabilities compared.

*Definition of done:* State error under 1e-4; mode probabilities within 1e-3.

*Data used:* Scenario-crate generated (CV→CT transition), supplemented with real
ADS-B/OpenSky flight tracks as an independent real-world reference.

*Risk if wrong:* Slow/incorrect mode-probability convergence means the tracker
"waits too long" to recognize a maneuver, producing a lagging track exactly when it
matters most.

**Square-root / UDU-Factorized KF & EKF**

*What it does:* Propagates covariance in square-root or UDU-factorized form instead
of directly, guaranteeing PSD covariance even under numerical stress.

*Why it matters:* Standard-form covariance can drift non-PSD after many cycles or
under ill-conditioned geometry, silently corrupting downstream estimates — this is
the insurance policy for long-running or numerically stressed deployments.

*How it's verified:* Functional comparison against the standard-form filter, plus a
dedicated soak test asserting PSD covariance every cycle over an extended run.

*Definition of done:* Under 1e-6 vs. standard-form; zero PSD violations over 10⁵+
cycles.

*Data used:* Synthetic fixture (functional) + scenario-crate adversarial-geometry
soak scenario (stability).

*Risk if wrong:* This is the capability specifically meant to prevent the
worst-case numerical failure mode in the product; a bug here defeats the purpose of
having a hardened filter at all.

**RTS Smoother / Fixed-Lag Smoothing**

*What it does:* A backward pass over an already-filtered trajectory (Rauch-Tung-
Striebel) using future information to improve past state estimates.

*Why it matters:* Forward filtering is causal; for post-hoc analysis (accident
reconstruction, track review) smoothing gives a materially better estimate of where
a target actually was.

*How it's verified:* Forward filter + smoother pass compared against
`filterpy.kalman.rts_smoother`. No canonical MATLAB reference exists.

*Definition of done:* Relative error under 1e-6.

*Data used:* Scenario-crate long-duration adversarial/soak scenario, supplemented
with ADS-B/OpenSky tracks (note: "truth" there is itself a smoothed reference).

*Risk if wrong:* Because smoothing feeds downstream reporting/post-incident
analysis, an error here could produce confidently wrong "official" reconstructions
with no forward-pass safety net.

### `gungnir-association` — Measurement-to-Track Association

**Nearest-Neighbor / GNN**

*What it does:* Assigns each detection to the single most likely track via global
nearest-neighbor cost minimization.

*Why it matters:* The simplest, cheapest strategy and the default whenever
ambiguity is low — most of the time, in most deployments, this is the algorithm
doing the work.

*How it's verified:* Same cost matrix through the Rust implementation, a
`scipy.linear_sum_assignment`-backed Python reference, and MATLAB's
`trackerGNN`/`assignauction`.

*Definition of done:* Exact match.

*Data used:* Synthetic fixture — a hand-constructed cost matrix.

*Risk if wrong:* An off-by-one or tie-breaking bug here misassigns detections in
the common case, not just the edge case.

**Hungarian / Jonker-Volgenant**

*What it does:* The optimal bipartite-matching solver underlying GNN association,
including degenerate/non-square cost matrices.

*Why it matters:* The combinatorial-optimization engine GNN — and indirectly other
strategies — relies on to solve the assignment problem correctly and efficiently.

*How it's verified:* Same cost matrix (including degenerate/rectangular cases)
through the Rust solver, `scipy.optimize.linear_sum_assignment`, and MATLAB's
`assignjv`.

*Definition of done:* Exact match on total cost; assignment matches only when not
tied.

*Data used:* Synthetic fixture, deliberately including degenerate/rectangular
matrices.

*Risk if wrong:* A solver bug corrupts every algorithm built on top of it, and
rectangular/degenerate-matrix bugs are exactly the kind that pass casual testing and
fail in production.

**Gating (Ellipsoidal / Chi-Square)**

*What it does:* Filters out statistically implausible detections before
association, given a track's predicted position and uncertainty.

*Why it matters:* The first line of defense against clutter/noise; every downstream
algorithm depends on gating having correctly narrowed the candidate set.

*How it's verified:* Same predicted state/covariance and measurements through the
Rust gate, a custom chi-square implementation, and MATLAB's internal
`trackingEKF`/`trackingGNN` distance function; gate membership compared.

*Definition of done:* Exact match.

*Data used:* Synthetic fixture.

*Risk if wrong:* A gate too loose lets clutter into JPDA/MHT downstream; too tight
silently drops legitimate detections, causing missed tracks.

**JPDA**

*What it does:* Joint Probabilistic Data Association — computes probability-
weighted associations between multiple tracks and ambiguous detections
simultaneously rather than a single hard assignment.

*Why it matters:* What the product needs once clutter density or target proximity
is high enough that greedy nearest-neighbor makes outright wrong assignments — the
difference between graceful degradation in clutter and not.

*How it's verified:* Per-track association probabilities vs. Stone Soup's `JPDA`
hypothesiser and MATLAB's `trackerJPDA`, on a shared clutter scenario.

*Definition of done:* Relative error under 1e-3.

*Data used:* Scenario-crate maritime clutter scenario (tuned harder), supplemented
with Stone Soup's own example JPDA scenario files.

*Risk if wrong:* Incorrect probability weighting under ambiguity only shows up
statistically over many scenarios, making it easy to ship undetected.

**Multi-Hypothesis Tracking (MHT)**

*What it does:* Maintains multiple competing association hypotheses over time,
pruning unlikely branches as evidence accumulates.

*Why it matters:* In the highest-clutter environments, even JPDA's single-step
blending isn't enough; MHT holds open multiple plausible explanations across time.

*How it's verified:* Hypothesis tree structure and top-N scores vs. Stone Soup's MHT
hypothesiser and MATLAB's `trackerTOMHT`.

*Definition of done:* Relative error under 1e-3 on scores; tree structure matches at
each pruning step.

*Data used:* Scenario-crate maritime clutter scenario, harder operating point.

*Risk if wrong:* A pruning-step error compounds — an incorrect early prune can
silently eliminate the branch that would have become the correct track.

### `gungnir-track` — Track Lifecycle

**Track Lifecycle (Init / Confirm / Coast / Delete)**

*What it does:* Governs the state machine deciding when detections become a
confirmed track, when a track with no recent detections is "coasted" rather than
deleted, and when it's finally dropped.

*Why it matters:* Determines whether the product reports a real, sustained track
versus noise, and whether a track survives a brief dropout instead of being
incorrectly terminated and re-initiated as "new."

*How it's verified:* Identical detection/miss sequence through the Rust track
manager, Stone Soup's initiators/deleters, and MATLAB's
`trackHistoryLogic`/`trackScoreLogic`; exact step index of confirm/delete
transitions compared.

*Definition of done:* Exact match on step index.

*Data used:* Scenario-crate maritime clutter scenario's land-mask dropout.

*Risk if wrong:* A coast/delete threshold error either drops real tracks
prematurely (losing history) or keeps stale tracks alive too long, cluttering the
operator picture.

### `gungnir-rfs` — Random Finite Set Filters

**PHD / CPHD Filter**

*What it does:* Tracks the *number* of targets and their collective intensity
function directly rather than maintaining individual identities.

*Why it matters:* Traditional track-per-object approaches degrade in dense scenes
where target count itself is uncertain — PHD/CPHD answers "how many objects are
there" as well as "where are they."

*How it's verified:* Both filters are built and gated (2026-09-06 PHD, 2026-09-08
CPHD). Stone Soup 1.9.1, the pinned library this row originally planned to verify
against, turned out not to be usable for either half, for two different reasons:
its PHD updater disagrees with the textbook recursion (a confirmed defect in its
mixture-reduction step), and it has no CPHD updater at all. Both rows are gated
against this crate's own hand-derived recursions instead -- the PHD one checked
against the textbook Vo-Ma paper, the CPHD one independently checked against a
brute-force enumeration of every possible detection-to-target association before
being trusted.

*Definition of done:* Weights within 1e-3; exact cardinality where unambiguous.

*Data used:* Scenario-crate dense-swarm scenario, replayed against the hand-derived
recursions above rather than the Stone Soup example scenarios originally planned,
since the library itself is not the oracle for either filter.

*Risk if wrong:* A cardinality-estimation error is business-visible — it means the
product reports the wrong *number* of objects present.

**GLMB / LMB Filter**

*What it does:* Like PHD/CPHD but additionally maintains persistent target
*identity* (labels) alongside cardinality.

*Why it matters:* Critical when downstream consumers need to reason about
individual object trajectories in a dense field, not just aggregate counts.

*How it's verified:* Same scenario vs. Stone Soup's (partial) GLMB and MATLAB's
`trackerGLMB`; label-to-track assignment and existence probabilities compared.

*Definition of done:* Existence probability within 1e-3; label continuity must
match.

*Data used:* Scenario-crate dense-swarm scenario, supplemented with Vo et al./
RFS-toolbox companion files.

*Risk if wrong:* A label-continuity bug produces "identity switching" — reporting
object A became object B — often more operationally damaging than positional error.

### `gungnir-fusion-async` — Asynchronous Multi-Sensor Fusion

**Out-of-Sequence Handling / Multi-Rate Fusion**

*What it does:* Correctly incorporates late/out-of-order detections and reconciles
sensors reporting at different, uncoordinated rates.

*Why it matters:* Any deployment with more than one sensor type encounters this in
normal operation, not as an edge case.

*How it's verified:* Replay a fixed multi-sensor timeline through the async
pipeline vs. an offline batch computation of the same timeline, using Stone Soup's
thin OOS updater as a partial reference. No direct MATLAB equivalent.

*Definition of done:* State convergence within 1e-4; latency is not part of the
pass/fail criterion.

*Data used:* Urban multi-sensor convoy scenario, manufacturing genuine sensor
timing disagreement.

*Risk if wrong:* Easy to under-test with a synthetic single-delay case and still
fail in production, where real sensor disagreement patterns are more complex.

**Concurrency Correctness**

*What it does:* Verifies the async pipeline is free of data races under Rust's
concurrency model, using `loom`'s exhaustive interleaving exploration.

*Why it matters:* Concurrency bugs pass thousands of normal test runs and then fail
unpredictably in production; this catches that category before shipping.

*How it's verified:* `loom` exhaustive interleaving against the urban convoy
scenario's actual pipeline, not a synthetic stress harness.

*Definition of done:* Zero races detected.

*Data used:* N/A — code-path exploration, not data-driven.

*Risk if wrong:* One of two capabilities in the whole system requiring mandatory
human sign-off in addition to a green CI check (the other being any `unsafe` block)
— reflecting that an undetected concurrency bug can cause silent data corruption
extremely difficult to diagnose after the fact.

### `gungnir-track-fusion` — Multi-Sensor Track Fusion

**Track-to-Track Fusion (CI, Information-Matrix)**

*What it does:* Combines multiple independent local tracks of the same real-world
object into a single, more accurate global estimate via covariance intersection or
information-matrix fusion.

*Why it matters:* Lets a multi-sensor deployment produce one coherent picture
instead of several conflicting per-sensor pictures — the normal case for any
deployment with more than one platform.

*How it's verified:* Identical local tracks through the Rust fusion implementation,
Stone Soup's (partial) fuser plus a hand-derived CI reference, and MATLAB's
`trackFuser`.

*Definition of done:* State error under 1e-6; covariance Frobenius-norm difference
under 1e-6.

*Data used:* Synthetic fixture.

*Risk if wrong:* A fusion bug can make a multi-sensor system perform *worse* than a
single sensor alone.

**Sensor Registration / Bias Estimation**

*What it does:* Estimates and corrects unknown systematic position/orientation bias
between sensor platforms.

*Why it matters:* Real platforms are never perfectly registered; without correction,
fusion confidently combines two individually-accurate but systematically offset
tracks.

*How it's verified:* Inject a known bias by construction, confirm recovery, vs. a
hand-derived least-squares reference and MATLAB registration functions.

*Definition of done:* Recovered bias within 1e-3 of injected ground truth.

*Data used:* Synthetic fixture with bias injected by construction (reuses the urban
convoy scenario data already producing multi-sensor tracks).

*Risk if wrong:* An unrecovered bias doesn't cause an obvious failure — it causes a
subtly, persistently wrong fused track that looks plausible.

### `gungnir-coord` — Coordinate Frame Transforms

**Coordinate Frame Transforms (ECEF/ENU/NED/Geodetic)**

*What it does:* Converts positions between Earth-Centered-Earth-Fixed, East-North-
Up, North-East-Down, and geodetic frames.

*Why it matters:* Coordinate transform bugs are a historically common source of
tracking-library failures, specifically at antimeridian crossings and near the
poles.

*How it's verified:* Round-trip transform plus cross-library comparison against
`pymap3d` and MATLAB's `ecef2enu`/`geodetic2ecef`, on a coordinate grid.

*Definition of done:* Position error under 1e-6 meters (or 1e-9 radians).

*Data used:* Synthetic fixture, deliberately including pole and antimeridian edge
cases.

*Risk if wrong:* Sits underneath every other capability in the system — a bug here
could masquerade as a filter, association, or fusion bug everywhere else.

### `gungnir-allocation` — Resource Assignment

**Bellman/DP Resource-to-Track Assignment**

*What it does:* Solves the resource-to-track assignment problem as a dynamic-
programming optimization over a reward/cost structure and planning horizon.

*Why it matters:* In any deployment with more sensors/resources than can be
dedicated to every track simultaneously, this decides where to point limited
resources for maximum tracking value.

*How it's verified:* Same reward/cost matrix and horizon through the Rust DP
implementation, a textbook-verified custom Python DP, and (if used) an equivalent
MATLAB DP.

*Definition of done:* Exact match on the value function (1e-9).

*Data used:* Synthetic fixture.

*Risk if wrong:* A DP bug doesn't produce an obviously wrong track — it produces a
subtly suboptimal policy, silently wasting resources.

### `gungnir-scenario` — Ground-Truth & Sensor Simulation

**Ground-Truth & Sensor Simulation (Statistical Self-Check)**

*What it does:* Generates synthetic ground-truth trajectories and simulated sensor
detections — including configurable Pd and clutter/false-alarm rate — that serve as
input for roughly half of all other capabilities in this table.

*Why it matters:* Every scenario-generated test elsewhere implicitly trusts the
generator's Pd and clutter rate are what they claim. If the generator is biased,
every test built on it inherits that bias silently.

*How it's verified:* Statistical comparison of empirical detection/clutter rates
over many trials vs. configured parameters, using Stone Soup simulators and MATLAB's
`trackingScenario`/`objectDetectionGenerator`.

*Definition of done:* Empirical rates within 2σ of configured values.

*Data used:* Self-generated — rides along "for free" as a side effect of the
maritime clutter scenario.

*Risk if wrong:* Arguably the highest-leverage single capability — a biased
generator silently invalidates every downstream test that depends on it.

**Round-Trip Fidelity (Generate → Export → Replay)**

*What it does:* Verifies a synthetic scenario generated in-memory can be exported,
replayed via `ReplayAdapter`, and reconstructed identically.

*Why it matters:* Any workflow recording a scenario for later replay depends on
this being lossless.

*How it's verified:* Generate → export → replay → compare against the original
in-memory stream. No external oracle.

*Definition of done:* Exact match on measurement content and arrival order; timing
within the export format's precision.

*Data used:* Self-generated, deliberately not tied to any of the five narrative
scenarios.

*Risk if wrong:* Undermines trust in recorded test cases and regression fixtures.

### `gungnir-metrics` — Tracking Performance Metrics

**MOTA/MOTP, Purity/Fragmentation, Track-to-Truth Assignment**

*What it does:* Computes standard MOT accuracy metrics by assigning tracker output
to ground truth and scoring the result.

*Why it matters:* The industry-standard way to communicate tracking quality to
stakeholders and compare against competing systems.

*How it's verified:* Identical tracker output and ground truth through the Rust
implementation, `motmetrics` (Python), and MATLAB's track-metric functions.

*Definition of done:* Metric values within 1e-3.

*Data used:* Public benchmark (MOT16/17/20-format) or scenario-crate dense-swarm
data.

*Risk if wrong:* A computation bug could mean the product is marketed or
benchmarked on numbers that don't reflect real performance.

### Cross-Cutting Capabilities

**End-to-End Tracking Accuracy**

*What it does:* Measures the complete pipeline's accuracy against externally
validated public benchmarks using OSPA/GOSPA distance metrics.

*Why it matters:* Answers "does the whole system actually work," independent of any
single module's unit-level test.

*How it's verified:* Full pipeline vs. Stone Soup/`motpy` on MOT16/17/20 and KITTI
tracking.

*Definition of done:* Within 5% of the reference implementation's score.

*Data used:* Public benchmarks only, deliberately kept separate from scenario-crate
output.

*Risk if wrong:* The capability most visible to external stakeholders — a
regression here has direct reputational impact even if every module's tests are
green.

**Numerical Stability Under Sustained Operation**

*What it does:* Runs the full pipeline for 10⁵+ cycles, asserting PSD covariance
and no NaN/Inf at every step.

*Why it matters:* Some numerical failure modes only manifest after sustained
operation — exactly the condition a production deployment will actually experience.

*How it's verified:* 10⁵+ cycle soak test, per-step PSD/NaN/Inf assertion.

*Definition of done:* Zero PSD violations, zero NaN/Inf, across the entire run.

*Data used:* Scenario-crate adversarial/ill-conditioned geometry scenario.

*Risk if wrong:* Hardest to catch in normal development, most damaging in
production — a long-running deployment that silently degrades has no natural point
where the problem becomes obvious before results are already wrong.

**Performance (Latency/Throughput)**

*What it does:* Benchmarks critical code paths via `criterion`, compared against
`filterpy`/`motpy` (Python) and MATLAB `codegen` (context only).

*Why it matters:* Correctness alone isn't sufficient — a provably accurate filter
too slow to keep up with sensor rate isn't deployable.

*How it's verified:* `criterion` runs vs. reference implementations for context.

*Definition of done:* Not pass/fail at the table level — regression is separately
gated via `bench-regression.yml`'s hard-fail p99 check.

*Data used:* Scenario-crate or public benchmark data, for realistic-scale timing.

*Risk if wrong:* Regression going unnoticed between releases if the CI gate isn't
kept in sync with this row's intent.

**Scenario/Interop Schema (JSON/Arrow, ASTERIX/STANAG)**

*What it does:* Validates the shared data schema round-trips through
serialize/deserialize without data loss, and validates against a corpus of
scenarios.

*Why it matters:* ASTERIX and STANAG are established surveillance/defense
interoperability standards; correct schema handling is what lets this product
exchange data with other systems and tooling.

*How it's verified:* Round-trip serialize/deserialize, validated against a corpus
of recorded and synthetic scenarios.

*Definition of done:* Zero data loss on round-trip; 100% schema validation pass
against the corpus.

*Data used:* A corpus of recorded and synthetic scenarios, chosen for format
breadth.

*Risk if wrong:* Threatens interoperability with external systems — may not surface
internally at all, only when integrating with a third party's compliant system.

---

## 2. Service Layer

*Crates: `gungnir-tracking-service`, `gungnir-intercept-service`.*

### Live Tracking Service

*What it does:* Takes raw sensor detections in and produces a continuously updated
track list out — the single door through which every other part of the application
(3D view, dashboard, intercept planner) learns "what's out there right now."
Internally wires together the filter bank, association/gating, track lifecycle, and
multi-sensor fusion crates from Part 1, but nothing outside this capability needs to
know that.

*Why it matters:* Every other feature is downstream of this working correctly and
staying current. If it lags or drops detections silently, every screen looks fine
while quietly showing stale or wrong information.

*How it's delivered:* A single `TrackingService` trait backed by the independently-
verified `gungnir-*` crates from Part 1 — the application gets the benefit of that
verification stack without needing to know those crates exist.

*Definition of done:* Reports a track list that never blocks the UI thread,
degrades visibly (a health flag) rather than silently when the pipeline falls
behind, and every track traces back to a Part 1 pass criterion.

*Data used:* Live sensor feeds in production; the five `gungnir-scenario` scenarios
in test/staging.

*Risk if wrong:* Because this is the single choke point for "where are the
targets," a bug here doesn't stay contained — it silently propagates into every
downstream screen simultaneously, and would likely be diagnosed as three separate
bugs before anyone traced it back to one cause.

*Status:* The facade re-exports `gungnir_model::TrackView` and `DetectionView` and
projects the core's kinematic track into the canonical view (`ARCHITECTURE.md`
§7.2). `LiveTrackingService` starts, accepts detections, and reports itself
unhealthy until the fusion pipeline is implemented, so the dashboard never claims
a working tracker.

### Intercept Planning Service

*What it does:* Given the current track list and a pool of available resources,
produces a recommended assignment — which resource should respond to which track,
and roughly when it would reach it.

*Why it matters:* Turns "we can see the target" into "here's what to do about it."
In any deployment with more tracks than resources, this is a genuine scheduling
decision with operational and cost consequences.

*How it's delivered:* Wraps `gungnir-allocation`'s Bellman/DP solver (independently
verified to an exact value-function match, per Part 1) behind a service interface
that also converts the raw assignment into something displayable.

*Definition of done:* A plan is produced without blocking the interface, degrades
to "last known good plan" rather than freezing if a solve takes too long, and every
solution is traceable back to the reward/cost inputs that produced it.

*Data used:* Live track output from the Tracking Service, plus an operator- or
config-supplied resource pool.

*Risk if wrong:* Unlike a tracking error (visibly wrong — a track in the wrong
place), an allocation error is often invisible — a subtly suboptimal plan still
looks reasonable. This kind of bug costs real operational value for a long time
before anyone notices.

*Status:* The facade re-exports `gungnir_model::PlanView`; `DpInterceptService`
keeps the last good plan, never tasks an unready resource, and reports itself
unhealthy when a solve fails. **The allocator solves as of 2026-09-06** (GAP-029): an
exact dynamic program over the horizon, gated against a textbook Python DP at every
state of the value function. Intercept geometry (the point and time the UI draws) is
computed from the assignment it returns (GAP-031).

---

## 3. 3D Data Ecosystem

*Crates: `gungnir-data`, `gungnir-data-fusion`. (Streaming large-scale tile support
lives as a `streaming/` module inside `gungnir-viewport3d` rather than its own
crate — see Part 4.)*

### Point Cloud / Scientific Mesh / Terrain / Asset Ingestion

*What it does:* Loads point-cloud (LAS/LAZ/COPC), scientific-mesh (VTK), terrain
(DEM), and 3D-asset (glTF) files from disk — background/context geometry for the
situational-awareness picture, separate from live track data.

*Why it matters:* Track dots on a blank screen are hard to interpret. Terrain,
structures, and background geometry are what make the 3D view a situational-
awareness tool rather than an abstract plot.

*How it's delivered:* Adopts existing, mature open-source libraries per file
format, converting each into a small internal data type so the rest of the
application never knows which library loaded a given file. All loading happens off
the interactive thread.

*Definition of done:* A supported file opens without freezing the UI, renders
correctly, and a corrupted or unsupported file produces a visible error rather than
a silent failure or crash.

*Data used:* Operator-supplied files (terrain tiles, facility models, sensor-
coverage meshes) relevant to the deployment site.

*Risk if wrong:* A loader that silently drops or misrepresents part of a terrain or
structure file gives operators a false sense of the physical environment.

### GPU-Accelerated Point Cloud Registration & Fusion

*What it does:* Aligns and merges point-cloud data from multiple sensors into one
consistent picture in real time, on the GPU so it doesn't compete with sensor
processing or the render loop for CPU time.

*Why it matters:* The one piece of this ecosystem with **no existing off-the-shelf
equivalent** — every other data-ingestion capability adopts a mature library; this
one is genuinely new engineering.

*How it's delivered:* A CPU reference implementation (`cpu_reference.rs`) is built
and validated first against known synthetic test cases, then the GPU compute
pipeline (`wgpu` compute shaders on a headless device owned by `gungnir-render`) is
validated against that reference. The CPU version is also kept as a runtime fallback
for hardware lacking the GPU capability, which matters for disconnected edge
deployments (`ARCHITECTURE.md` §8.7). Fused output is read back to the CPU for
display, because the 3D viewport draws through a separate OpenGL context.

*Definition of done:* Registration converges to a known-correct alignment on test
data within tolerance, never blocks the render loop, and falls back visibly to the
last good alignment on non-convergence rather than showing a corrupted cloud.

*Data used:* Synthetic point clouds with a known, injected transform (correctness
testing); live multi-sensor feeds in production.

*Risk if wrong:* Produces a merged point cloud that looks plausible but is subtly
misaligned — hard to catch visually but means two sensors' data disagree about
exactly where something physically is.

*Cross-reference:* Nothing currently passes this crate's registration output
(transform, uncertainty, calibration version) into `gungnir-track-fusion`'s sensor
registration/bias estimation (Part 1) as evidence — see Principal Risks, §9.

---

## 4. UI / Application

*Crates: `gungnir-render`, `gungnir-viewport3d`, `gungnir-ui`, `gungnir-app`.*

### Real-Time Track Dashboard

*What it does:* A 2D, always-current list/table view of every track the Tracking
Service currently reports.

*Why it matters:* The 3D view is good for spatial context but poor for "give me the
full list, sorted, right now" — this is how an operator keeps track of everything
at once as track count grows.

*How it's delivered:* Reads directly from the Tracking Service's current snapshot
every frame; never holds its own copy, so the dashboard and 3D view can never
disagree about what's currently true. The track table, intercept panel, system
health, and alert panels are implemented in `gungnir-ui` and wired in
`gungnir-app`; the accept/override/reject controls arrive with the approval gate.

*Definition of done:* Reflects the Tracking Service's state with no perceptible
lag, stays readable and responsive as track count scales.

*Data used:* Live output of the Tracking Service.

*Risk if wrong:* If dashboard and 3D view ever held separate copies of track
state, they could silently disagree — an operator trusting one could see different
information than one trusting the other. This risk is why the design prevents it
rather than mitigating it after the fact.

### 3D Situational-Awareness Viewport

*What it does:* Renders tracks, terrain, sensor-coverage geometry, and intercept
geometry together in one interactive 3D scene with camera controls.

*Why it matters:* The primary "big picture" screen — where operators build spatial
intuition a table of numbers alone can't provide.

*How it's delivered:* GPU-accelerated rendering through `three-d` in the
application's OpenGL context, with track/intercept geometry rebuilt only when
underlying data actually changes, keeping the view responsive as track count and
data volume grow. The viewport is called every frame; until the three-d scene is
attached to the GL context it draws a top-down 2D projection of the same glyphs,
with pan and zoom and a metric grid (`ARCHITECTURE.md` §4).

*Definition of done:* Sustains an interactive frame rate at the deployment's
expected data volume; a rendering error in one element (e.g., a bad terrain tile)
degrades that element rather than crashing the whole view.

*Data used:* Live Tracking/Intercept Service output; loaded terrain/scientific/asset
data.

*Risk if wrong:* A dropped frame or stall here is operationally worse than
elsewhere in the application, since this is the screen operators are most likely
actively watching moment-to-moment.

*Note:* This is also the one crate that intentionally sees tracking-service output
types directly (`gungnir-viewport3d::tracks`) — a 3D track glyph is a rendering
concern, so this avoids an unnecessary translation crate. See `ARCHITECTURE.md` §5.

### Intercept Planning Panel

*What it does:* Displays the Intercept Planning Service's current recommended
assignments in a form an operator can act on, alongside the 3D view's rendering of
the same plan as geometry.

*Why it matters:* A recommendation an operator can't quickly read and evaluate
isn't useful under time pressure.

*How it's delivered:* Reads directly from the Intercept Planning Service's current
plan; same single-source-of-truth discipline as the dashboard.

*Definition of done:* Panel and 3D intercept geometry always agree, since both read
the same underlying plan.

*Data used:* Live output of the Intercept Planning Service.

*Risk if wrong:* A stale or mismatched plan shown to an operator could lead to
acting on an assignment that's no longer current.

### Sensor Health & Alerting

*What it does:* Surfaces the tracking pipeline's health/connectivity and a running
list of operationally relevant alerts.

*Why it matters:* Silent degradation is the most dangerous failure mode in a
real-time tracking system — a pipeline quietly falling behind but still producing
*some* output looks fine until it very much isn't.

*How it's delivered:* Surfaces health/status signals the Tracking and Intercept
Services already expose by design, rather than inferring health indirectly.

*Definition of done:* Every degraded-but-recoverable condition the underlying
services can report has a corresponding visible signal, not just hard-failure
cases.

*Data used:* Live health/status output from the Tracking and Intercept Services.

*Risk if wrong:* If this under-reports degraded states, operators lose the one
mechanism that would otherwise warn them the picture they're trusting is no longer
fully reliable.

---

## 5. Productization Layer

An independent architecture review of Parts 1–4 concluded they form a strong
*tracking, fusion, visualization, and allocation engine* — not yet a complete
*operational solution*. This section covers the twenty-one crates added to close
that gap and the five added on 2026-09-04 (`gungnir-interop`, `gungnir-analytics`,
`gungnir-resilience`, `gungnir-collab`, `gungnir-workflow`), organized the same way
the review organized them: five capability domains plus Foundational. The two
deployment crates (`gungnir-remote`, `gungnir-node`) appear in the crate map (§8)
and in `ARCHITECTURE.md` §8.

**Implementation status for this whole section:** every crate below compiles under
the workspace lint policy, has a doc-commented trait surface, and, where it has
logic, a working in-memory implementation with unit tests (the per-crate status
lines say which). Wired into the desktop and node tick loops today: `gungnir-model`,
`gungnir-eventing`, `gungnir-store`, `gungnir-config`, `gungnir-mission` (a live
session), `gungnir-time`, `gungnir-ingest`, and the health reporting
`gungnir-observability` defines. `gungnir-policy` and `gungnir-command` (the
approval gate) are wired into the desktop tick as of 2026-09-05 (GAP-038) and into the
node's nowhere, and `gungnir-replay` and `gungnir-reporting` are read by the desktop's
PN-12 and PN-13 (GAP-071), and `gungnir-sensor-management` is constructed by both
binaries (GAP-003) and carries the outbound control path as of the same day (GAP-004),
with no adapter attached to carry a command off the machine, so every command issued
stops at `Issued` and times out unacknowledged (GAP-001). Implemented but not yet wired: `gungnir-assessment` (real
rewards),
`gungnir-identity`, `gungnir-identification`,
`gungnir-replay`, `gungnir-reporting`, and `gungnir-analytics`. `gungnir-workflow` left
that list on 2026-09-05: its role layouts drive the desktop's workspace (GAP-055) and its
tasking case is behind PN-15 (GAP-005). `gungnir-security` left it the same day: an
operator can sign in against a local account store and what the system records about who
acted follows from that (GAP-057, DN-23). A deployment with no account store still starts
and still attributes nothing, which is the state the default baseline is in.
`verification-capability-table.md` §2 holds a row for each; "Tested" rows have
their invariant tests, "Draft" rows still need a pass criterion agreed before the
crate is "done" in the same sense as Part 1.

### 5.1 Foundational

**Canonical Operational Data Model** — `gungnir-model` — *Critical*

*What it does:* Defines one shared, versioned set of domain types —
`TrackView`/`ResourceView`/`PlanView`/`SystemHealth` and the `TrackingEvent`/
`InterceptEvent` schema — with provenance, quality, identity, timestamps, and
classification as first-class fields.

*Why it matters:* Without one canonical model, every other new capability
(ingestion, identity, persistence, decision support) either invents its own
overlapping notion of "what a track is," or gets bolted onto the wrong owner. This
is the highest-leverage crate in this entire section — 17 of the other 20 depend on
it directly.

*Status:* Implemented and tested: `TrackView`, `DetectionView`, `ResourceView`,
`PlanView`, `SystemHealth`, the four event enums, and the schema-version check, all
with serde round-trip tests. Both service facades re-export its types
(`ARCHITECTURE.md` §7.2); wired into `gungnir-app::AppState` and `gungnir-node`.

**Event & Durable Messaging** — `gungnir-eventing` — *High*

*What it does:* A transport-neutral event interface (in-process channel today, a
durable/replayable log or network stream later) that services publish to and
subscribers (UI, future persistence/API) read from, instead of polling a mutable
slice every frame.

*Why it matters:* Polling works for a single-window desktop app; it doesn't
support persistence, replay, multi-user collaboration, or external integration —
all of which need "here's what changed and when."

*Status:* Implemented and tested: `InProcessBus` broadcasts stamped `Envelope`s to
every subscriber in one global sequence order, prunes dropped subscribers, and
tolerates having none. Wired: the desktop and the node publish ingest and plan
events and journal every envelope through a subscriber.

**Persistence & Data Lifecycle** — `gungnir-store` — *High*

*What it does:* Durable storage for mission sessions, replay artifacts, geospatial
caches, configuration baselines, and audit logs, with explicit retention/deletion
policy.

*Why it matters:* Once a session ends today, nothing about it persists. Reviewing a
past session or reproducing an incident depends on this existing.

*Status:* Implemented and tested: `FileEventJournal` writes one JSON-lines file per
session, tolerates a torn final line after a crash, and lists sessions;
`RetentionPolicy` has defaults. Wired: both binaries open a journal at startup and
append every envelope the bus carries.

**Configuration & Mission Management** — `gungnir-config`, `gungnir-mission` —
*High*

*What it does:* A managed, versioned, validated configuration model for sensors,
filter/association selection, thresholds, allocation weights, and terrain layers
(`gungnir-config`), plus session lifecycle — create/load/save/replay/close
(`gungnir-mission`).

*Why it matters:* Sensors/resources are currently assumed to be supplied
programmatically. Any real deployment needs to add or reconfigure a sensor without
a code change and rebuild.

*Status:* `gungnir-config` implemented and tested: `FileConfigStore`, validation,
version gating, forward-compatible loading, sensors and resources, backend
selection (embedded or remote), and node settings. `gungnir-mission` remains a
trait surface; both binaries open a live `Mission` at startup and journal under
its session id.

### 5.2 Sense, Ingest & Normalize

**Real Sensor & External-System Integration** — `gungnir-ingest` — *Critical*

*What it does:* Protocol-specific adapters plus a validation/authentication/
quarantine gateway that normalizes live, recorded, and simulated sources into one
canonical observation envelope before anything reaches the Tracking Service.

*Why it matters:* Without this, "detection" is only an internal test-fixture
concept. Malformed, malicious, or out-of-spec input needs to be caught here, not
inside the tracking core.

*Status:* Implemented and tested: the gateway authenticates sources against the
configured allow-list, validates every observation, quarantines what fails, and
forwards the rest; recorded (JSON-lines, released in source-time order) and
simulated adapters exist, and a fuzz target covers the parser. Wired into both
binaries' ticks. Live protocol adapters are still to be written, using the
`gungnir-interop` codecs.

**Sensor Management & Adaptive Collection** — `gungnir-sensor-management` — *High*

*What it does:* A sensor registry (identity, modality, location, calibration
version, operating state) plus tasking, mode management, and coverage planning.

*Why it matters:* The current design shows sensor health but has no way to
actually task, calibrate, or manage a sensor's operating mode.

*Status:* Implemented and tested: `InMemorySensorRegistry` built from the config
baseline, an explicit mode-transition table, and coverage regions from mode and
range. Not yet wired into a panel.

**Time Management & Synchronization** — `gungnir-time` — *Critical*

*What it does:* Clock discipline across sensors, source-time vs. receipt-time
semantics, late-data policy, deterministic replay-time control, and
synchronization health reporting.

*Why it matters:* Formalizes, application-wide, what `gungnir-fusion-async`'s
out-of-sequence handling already does internally — a deployed multi-sensor system
needs the same discipline consistent across ingestion, persistence, and replay.

*Status:* Implemented and tested; both binaries read mission time from a
`WallClockAuthority`, `gungnir-replay` drives a `ReplayClockAuthority`, and every
`DetectionView` carries source and receipt time. Cross-sensor clock-skew tracking
is not implemented, so synchronization health reports no sources out of sync.

**Data Standards & Interoperability** — `gungnir-interop` — *Medium*

*What it does:* A controlled schema catalog with version negotiation, the Arrow
columnar representation of detections, and the codec boundary that ASTERIX and
STANAG adapters implement — the governance behind the interop schema row in Part 1.

*Why it matters:* Interop formats are named as a goal; without an owned schema
catalog and compatibility rules, "we support ASTERIX/STANAG" is not yet a claim this
project could back up in an integration with a third-party system.

*Status:* Catalog and Arrow round-trip implemented and tested; the ASTERIX CAT048
and STANAG 4676 codecs are boundaries that return `NotImplemented`.

**Data Quality, Provenance & Confidence Governance** — *extends `gungnir-model` +
`gungnir-ingest`* — *High*

*What it does:* Attaches source lineage, calibration-baseline version, association
confidence, and freshness/latency to every track and observation, with quality
gates before data is displayed, fused, or used for allocation.

*Why it matters:* An operator needs to know not just "where is it" but "how much
should I trust this" — a stale, low-confidence track displayed with the same
visual weight as a fresh one is a real operational risk.

*Status:* `Provenance` and `Quality` are on every view and detection; stale tracks
are drawn muted by the UI and score zero in `gungnir-assessment`. The freshness
limit that sets `is_stale` is not yet applied anywhere — no dedicated crate, by
design (it extends the two crates named above rather than owning its own boundary).

### 5.3 Understand & Maintain the Operational Picture

**Identity Management & Track Continuity** — `gungnir-identity` — *High*

*What it does:* Persistent global track/entity identity across feeds — aliases,
merge/split history, supersession — beyond the tracking core's internal track IDs.

*Why it matters:* The tracking core manages IDs correctly *within one session*; a
multi-system or multi-session deployment needs identity that survives restarts,
replays, and external correlation.

*Status:* Implemented and tested: `InMemoryIdentityResolver` keeps a stable global
id per session track and records merges with lineage. Cross-session kinematic or
classification correlation is not implemented. Not yet wired.

**Classification & Identification (Friend/Foe/Unknown)** — `gungnir-identification`
— *Medium*

*What it does:* Assigns and maintains a classification per track from an
evidence-fusion process, feeding both the dashboard and the intercept planner's
candidate list.

*Why it matters:* Every track today is purely kinematic. In almost any real
deployment, "what is it" is at least as consequential as "where is it" — without
this, the Intercept Planning Service recommends resources against tracks
indiscriminately.

*Status:* Implemented and tested: `EvidenceFusionEngine` sums confidence per class
and stays `Unknown` unless the leader clears a decision margin. Not yet wired.

**Geospatial Reference & Map Service Layer** — `gungnir-geo` — *Medium*

*What it does:* Coordinate-reference-system governance, map/imagery layers,
geocoding, vector features, spatial indexing, and geofencing — extends
`gungnir-data`'s terrain-elevation-only ingestion with visual and functional
geospatial context.

*Why it matters:* Elevation alone gives shape without orientation cues; operators
generally orient far faster against recognizable imagery.

*Status:* Implemented and tested: `InMemoryGeoService` and great-circle geofences
that stay correct across the antimeridian; `gungnir-policy` uses `is_within_no_go`.
Map and imagery layers are descriptors only; nothing renders them yet.

**Advanced 3D Analytical Functions** — `gungnir-analytics` — *Medium*

*What it does:* Line-of-sight, viewshed, occlusion, sensor-coverage volumes,
terrain masking, and route deconfliction — analysis, not just display.

*Why it matters:* The viewport renders geometry but doesn't answer operationally
useful questions like "can this sensor actually see that location."

*Status:* Implemented as standalone CPU services, rendered by `gungnir-viewport3d`
rather than embedded in it, and tested: flat-terrain and terrain-mask line-of-sight
(a reference implementation, nearest-vertex sampling), viewshed, coverage volumes
with range and elevation limits, and route-through-no-go checks. Not yet wired to
a panel; GPU variants are future work.

### 5.4 Assess, Decide & Govern Action

**Safety / Authority Controls for Intercept Planning** — `gungnir-policy`,
`gungnir-command` — *Critical*

*What it does:* An authority model, human-approval workflow, geofence/no-go
enforcement, and an explicit decision boundary between "the system recommends" and
"a resource acts," sitting in front of the Intercept Planning Service's output.

*Why it matters:* The Intercept Service currently produces a plan; nothing governs
whether, how, or by whom that plan is authorized before anything happens. This is
the difference between a decision-support tool and an unsupervised action system.

*Status:* Both implemented and tested: `GeofencePolicy` denies plans inside a no-go
fence or tasking an unready resource and otherwise requires a human, `PolicyChain`
composes engines and never approves on its own, and `InMemoryApprovalWorkflow`
refuses denied plans and records every decision as an append-only history and a
`CommandEvent`. Not yet wired between `intercept` and `last_plan` in either binary,
so today's proposed plan reaches the screen without passing the gate; nothing acts
on it either.

**Threat / Risk Assessment** — `gungnir-assessment` — *Medium*

*What it does:* Formal threat/risk scoring, predicted trajectory/time-to-impact,
asset exposure, and confidence-aware prioritization feeding the allocation
decision.

*Why it matters:* `gungnir-intercept-service` currently optimizes a reward/cost
matrix it's *given* — nothing defines how those values are actually derived from
real threat/priority reasoning.

*Status:* Implemented and tested: `ClosingSpeedAssessor` scores proximity and
approach toward a protected point with time-to-impact, stale tracks score zero, and
`reward_matrix` shapes the planner's input. The planner still receives a uniform
matrix until this is wired.

**Decision Support Beyond DP Allocation** — `gungnir-decision` — *Medium*

*What it does:* Course-of-action generation and comparison, constraint handling,
what-if simulation against a live snapshot, and recommendation explanations.

*Why it matters:* A single optimization method is a starting point, not a complete
decision-support capability — real decisions usually need alternatives,
explainability, and "what if" before committing.

*Status:* `rationale_for` (explanations ordered by risk) implemented and tested;
`DecisionSupport` (ranked alternatives, what-if) remains a trait surface.

**Model Lifecycle & Algorithm Governance** — `gungnir-modelops` — *Medium*

*What it does:* A registry for selecting, tuning, comparing, validating,
promoting, and rolling back filter/association configurations by mission profile.

*Why it matters:* `gungnir-filters`/`gungnir-association` offer multiple algorithm
choices, but nothing manages which configuration is in use where, or governs
changing it safely.

*Status:* Implemented, tested and **wired 2026-09-05** (GAP-086, DN-24):
`InMemoryModelRegistry` with a validation gate before promotion and rollback to the
previously promoted baseline, keyed on `AlgorithmBaselineId` so a rollback can say which
candidate it restored. Both binaries build one from the baseline and journal what the
session opened with. What a promoted configuration cannot yet do is reach the picture --
there is no pipeline to apply it to (GAP-011, GAP-053).

### 5.5 Secure, Operate & Sustain

**Cybersecurity & Platform Hardening** — `gungnir-security` — *Critical*

*What it does:* Authentication, role/attribute-based authorization, encryption in
transit/at rest, secrets/key management, signed binaries and configuration, and
audit logging for configuration/algorithm/plan changes.

*Why it matters:* Nothing else in this workspace specifies identity, access
control, or supply-chain assurance. For a system ingesting external sensor feeds
and recommending physical-world actions, this is baseline, not a hardening pass.

*Status:* The role-to-action matrix, `StaticRoleAuthorizer`, and `InMemoryAuditLog`
are implemented and tested. The credential mechanism is undecided
(`Authenticator` is a trait only), and nothing in either binary gates access yet.

**System-of-Systems API Boundary** — `gungnir-api` — *High*

*What it does:* Versioned REST/gRPC/event-stream interfaces so remote clients,
external C2 systems, analytics tools, or enterprise data services can integrate
with the application.

*Why it matters:* The application is currently desktop-centric with no defined way
for anything external to consume its output or feed it data programmatically.

*Status:* The v1 contract is defined and documented (`gungnir-api-v1.md`): snapshot,
event subscription, detection submission, and plan decision, with per-call
authorization in the handler trait and serialization tests. The transport (JSON
over HTTP plus a WebSocket stream) is decided but not yet in the workspace;
`UnimplementedServer` says so at node startup, and `gungnir-remote` falls back to
embedded services on the desktop.

**Health, Observability & Operational Diagnostics** — `gungnir-observability` —
*High*

*What it does:* Production-facing metrics, structured logs, distributed traces,
health checks, watchdogs, and alert correlation — extends the developer-facing
`tracing` instrumentation into an operator-facing system-status capability.

*Why it matters:* `tracing` today answers "help a developer debug a specific test
failure." It doesn't answer "is this deployed system healthy right now" for
whoever keeps it running.

*Status:* `SnapshotHealthMonitor` (alert correlation by summary, highest severity
kept) implemented and tested; the node uses `WatchdogConfig` for its ingest-gap
warning, and both binaries report `SystemHealth` from what the services say, never
by inference. The node's health endpoint waits on the API transport
(`ARCHITECTURE.md` §8.3).

### 5.6 Validate Against Reality

**Reporting, After-Action Review & Export** — `gungnir-replay`,
`gungnir-reporting` — *Lower*

*What it does:* Deterministic session playback and timeline scrubbing
(`gungnir-replay`, reading `gungnir-store`'s journal through a
`gungnir-time::ReplayClockAuthority`), and mission reports/export packages with
provenance preserved (`gungnir-reporting`, also pulling `gungnir-metrics` scoring
where ground truth is available).

*Why it matters:* `gungnir-filters`'s RTS/fixed-lag smoothing already implies
post-hoc analysis is a valued use case; nothing currently packages that analysis
into something a stakeholder outside engineering could read or act on.

*Status:* Both implemented and tested against real journals: `ReplaySession` steps
and seeks with a clock that follows the envelopes, and `JournalReportGenerator`
recomputes event counts from the journal and exports JSON. Neither has a panel in
`gungnir-ui` yet.

**Human Factors & Workflow Design** — `gungnir-workflow` — *Medium*

*What it does:* Role-specific workspaces (operator, supervisor, analyst, sensor
manager, administrator), the alert acknowledgement/escalation/closure lifecycle,
annotations, and cases — turning a set of panels into a designed operator workflow.

*Status:* Implemented and tested: the panel set per role and an alert lifecycle
with enforced transitions and recorded history. `gungnir-ui` does not yet switch
layouts by role.

**Collaboration & Multi-User Concurrency** — `gungnir-collab` — *Medium*

*What it does:* A shared operational picture across the desktops connected to one
node (the node is the system of record; each desktop applies its envelopes), and
arbitration when two operators decide differently on the same plan.

*Status:* Implemented and tested: `InMemorySharedPicture` rejects stale envelopes,
and `RoleRankArbiter` lets the higher role win with the earlier decision winning a
tie. This default rule is working but not yet locked (`ARCHITECTURE.md` §10).

**Resilience & Disconnected Operations** — `gungnir-resilience` — *Medium*

*What it does:* Store-and-forward of envelopes while a node is unreachable,
checkpoints for recovery, and reconciliation of a desktop's offline journal with
the node's when the link returns (`ARCHITECTURE.md` §8.4).

*Status:* Implemented and tested: a bounded queue that drops and counts the oldest
when full, and `reconcile`, which merges by mission time, drops exact duplicates,
and reports conflicting decisions rather than resolving them. Mid-session failover
waits on the API transport's heartbeat.

**Deployment Topology, Scalability & Performance Budgets, Software Assurance &
Release Governance** — *decided or documented, no crate needed*

- *Deployment topology* — **decided and scaffolded.** Three profiles from one crate
  set: a disconnected desktop with the services embedded, and an on-prem or cloud
  service node (`gungnir-node`, a runnable headless binary targeting Linux x86_64
  containers, `deploy/`) hosting the same services behind `gungnir-api`, with
  `gungnir-remote` as the desktop's remote backends (`ARCHITECTURE.md` §8). Which
  crate group runs where is tabulated in §8.3. The API transport is the remaining
  piece.
- *Scalability & performance budgets* — **drafted.** End-to-end SLOs per profile,
  derived from the mission-scale scenarios, are in `performance-budgets.md` awaiting
  confirmation; the harnesses that measure them do not exist yet.
- *Software assurance & release governance* — **documented and automated.**
  `release-governance.md` with `deny.toml` and `release.yml`: license and advisory
  policy, SBOM, auditable builds, signed artifacts, the node container image.
  Registry, signed configuration baselines, and a vulnerability-response objective
  are still to be decided.

---

## 6. Independent Assessment: Is the Fusion Sound?

A structural review of Parts 1–4 reached a consistent verdict: **the fusion is a
strong technical integration baseline, but not yet a complete operational
solution.** It is a well-structured *tracking, fusion, visualization, and
allocation engine* — not yet a fully deployable mission application, sensor-
integration platform, governed decision-support system, or production product. The
productization layer in Part 5 is the direct response to that verdict.

What the review found sound, specifically:

| Area | Verdict |
|---|---|
| Keeping estimation/tracking math isolated from UI/GPU | Correct protection of numerical logic from presentation-layer coupling |
| The `gungnir-tracking-service` / `gungnir-intercept-service` boundary | The single most important decision in the design — the desktop app depends on stable operational objects, never on the internal composition of the tracking stack |
| Splitting `gungnir-data` (I/O, unit-testable) from `gungnir-data-fusion` (GPU-aware) | Sound — avoids GPU ownership, render state, and ingest logic becoming inseparable |
| The five-scenario verification strategy | A genuine differentiator — tests integrated failure modes rather than only isolated algorithms |
| `gungnir-render` owning the one `wgpu` device in the process | Correct resource-lifecycle discipline. (The review assumed the 3D viewport also drew through that device; it does not, since `three-d` renders through OpenGL. The single `wgpu` device now serves point-cloud compute only, and the verdict on lifecycle discipline stands. See `ARCHITECTURE.md` §9.) |

In short: the parts that exist are built the right way. The gap was coverage, not
quality — which is why Part 5 adds a product/platform backbone around the engine
rather than more tracking algorithms or UI panels.

---

## 7. Recommended Build Increments

A practical path through Part 5's scaffolded (but not yet implemented or wired)
crates, without disturbing Parts 1–4's already-sound boundaries.

**Increment 1 — Productize the core.** Implement `gungnir-model`, replace ad hoc
service outputs with the versioned snapshot/event contracts it defines, implement
`gungnir-config`/`gungnir-mission` session lifecycle, implement `gungnir-eventing`
and `gungnir-store`'s event journal for deterministic replay.
*Business exit criterion:* a user can load a recorded scenario, watch the live
picture, inspect track quality/history, get a recommendation, and replay the whole
session with the same result.

**Increment 2 — Integrate real data.** Implement `gungnir-ingest`, `gungnir-time`,
`gungnir-sensor-management`'s registry/calibration, source health scoring.
*Business exit criterion:* multiple real or recorded sources enter through
governed adapters, get normalized, and retain traceable provenance all the way to
the display.

**Increment 3 — Close the decision-support loop.** Implement
`gungnir-assessment`'s threat/risk scoring, `gungnir-policy`/`gungnir-command`'s
constraint/approval engines, `gungnir-decision`'s what-if execution, decision
explanations and audit trails, safe degraded behavior under low confidence.
*Business exit criterion:* the system recommends — never autonomously executes —
policy-constrained actions with a clear rationale and a complete audit trail.

**Increment 4 — Operationalize and scale.** Implement `gungnir-security`,
`gungnir-api`, `gungnir-observability`, and the `gungnir-node` service binary with
the desktop's remote backends; resolve the remaining collaboration and
reconciliation decisions; establish performance/capacity targets including the
connected profiles' latency budget.
*Business exit criterion:* the system supports all three deployment profiles
(`ARCHITECTURE.md` §8) with measurable reliability, performance, security, and
recoverability, and a desktop survives losing its node.

---

## 8. Full Crate Map

All 50 crates in `gungnir-workspace`, one row each (49 workspace members plus
`gungnir-fuzz`, which is excluded from the default build). "Trait surface" means the
public API exists with `todo!()` or `NotImplemented` bodies; "Implemented" means the
in-memory or file-backed implementation exists with unit tests; "Wired" means a binary
calls it in its tick loop.

| Crate | Layer | Business Capability | Priority | Status |
|---|---|---|---|---|
| `gungnir-core` | Tracking core | CV/CA/CT motion models; owns the shared id, status, and PSD-assertion primitives | Foundational | **Implemented and gated 2026-09-05**: CV, CA, and CT `F`/`Q`, agreeing with filterpy to 1.4e-14 against a 1e-10 criterion (MATLAB column not run, not installed); `assert_psd` implemented and tested |
| `gungnir-coord` | Tracking core | ECEF/ENU/NED/geodetic transforms | Foundational | **Implemented and gated 2026-09-05**: agreeing with pymap3d to 2.1e-9 m against a 1e-6 m criterion, including both poles and the antimeridian |
| `gungnir-filters` | Tracking core | KF/EKF/UKF/PF/IMM/sqrt-UDU/RTS | Foundational | **Linear KF implemented and gated 2026-09-05** (Joseph form, agreeing with filterpy to 9.1e-13); the other six are trait surfaces, and the EKF bench is still a placeholder |
| `gungnir-association` | Tracking core | NN/GNN, Hungarian/JV, gating, JPDA, MHT | Foundational | **Hungarian/JV, GNN, and chi-square gating implemented and gated 2026-09-05** against scipy; the assignment bench is real and the fuzz target now drives the solver. JPDA and MHT are trait surfaces |
| `gungnir-track` | Tracking core | Track lifecycle | Foundational | **Implemented and gated 2026-09-05**: init/confirm/coast/delete agreeing with Stone Soup on every confirm and delete step index |
| `gungnir-rfs` | Tracking core | PHD/CPHD, GLMB/LMB | Foundational | **PHD implemented and gated 2026-09-06, CPHD implemented and gated 2026-09-08** (§1 above); GLMB/LMB are trait surfaces, and the dense-swarm bench is still a placeholder |
| `gungnir-fusion-async` | Tracking core | OOS/multi-rate fusion, concurrency | Foundational | Ingest task runs and drains; pipeline not implemented (`PIPELINE_IMPLEMENTED = false`) |
| `gungnir-track-fusion` | Tracking core | Track-to-track CI fusion, registration/bias | Foundational | Trait surface |
| `gungnir-allocation` | Tracking core | Bellman/DP resource assignment | Foundational | Returns `NotImplemented`; degenerate inputs tested |
| `gungnir-scenario` | Tracking core | Five-scenario generator | Foundational | **Implemented 2026-09-05**: Scenarios 1 to 5 with truth, sensor model, and detections; both `scenario` verification rows gated. Does not reproduce the plan-07 Python generator byte for byte (GAP-016) |
| `gungnir-metrics` | Tracking core | MOTA/MOTP, purity/fragmentation | Foundational | **Implemented and gated 2026-09-05** against `py-motmetrics`, counts included |
| `gungnir-oracle` | Tracking core (verification) | Differential-test harness | Foundational | Harness surface |
| `gungnir-testkit` | Tracking core (verification) | Shared proptest strategies | Foundational | Implemented and tested |
| `gungnir-fuzz` | Tracking core (verification) | Fuzz targets (ingest parser, cost matrix) | Foundational | Targets implemented; nightly workflow |
| `gungnir-model` | Foundation | Canonical views, events, schema version | Critical | Implemented and tested; wired |
| `gungnir-tracking-service` | Service layer | Detections in, `TrackView`s out | Core | Implemented facade over an unimplemented pipeline; wired; health honest |
| `gungnir-intercept-service` | Service layer | Tracks and resources in, `PlanView` out | Core | Implemented facade over an unimplemented allocator; wired; health honest |
| `gungnir-eventing` | Productization / Foundational | Broadcast event bus with envelopes | High | Implemented and tested; wired |
| `gungnir-store` | Productization / Foundational | JSON-lines session journal, retention | High | Implemented and tested; wired |
| `gungnir-config` | Productization / Foundational | Managed configuration, backend and node settings | High | Implemented and tested; wired |
| `gungnir-mission` | Productization / Foundational | Session lifecycle | High | Implemented 2026-09-05 (GAP-051): `JournalMissionManager` over the journal directory; both binaries create, transition and close through it, and a session left unclosed reopens as `Interrupted` |
| `gungnir-ingest` | Productization / Sense-Ingest | Gateway, recorded and simulated adapters | Critical | Implemented and tested; wired. Live adapters built and gated: radar (ASTERIX), AIS, SAPIENT (spotter, acoustic, passive-RF), and ADS-B. STANAG 4676 is pinned and blocked on NSO access; ISR-video (motion imagery) is surveyed and deliberately not pinned, since it is a viewport concern rather than a gateway one; EO/IR has no pinned specification at all (GAP-001, GAP-064) |
| `gungnir-sensor-management` | Productization / Sense-Ingest | Sensor registry, modes, coverage | High | Implemented and tested |
| `gungnir-time` | Productization / Sense-Ingest | Clock discipline, replay-time control | Critical | Implemented and tested; wired |
| `gungnir-interop` | Productization / Sense-Ingest | Schema catalog, Arrow form, ASTERIX/STANAG codec boundary | Medium | Catalog and Arrow implemented and tested. Decoders built and gated 2026-09-06: ASTERIX Categories 048 and 034 to their pinned editions (GAP-064), AIS to ITU-R M.1371-6 (GAP-010, D-32), and ADS-B 1090 ES against the **open-source consensus with no normative source pinned** (GAP-010; `design/external-standards.md` §4). STANAG 4676 and every encode path still return `NotImplemented` |
| `gungnir-identity` | Productization / Understand | Global entity identity, lineage | High | Implemented and tested |
| `gungnir-identification` | Productization / Understand | Friend/foe/unknown classification | Medium | Implemented and tested |
| `gungnir-geo` | Productization / Understand | Geofences, map layers | Medium | Implemented and tested |
| `gungnir-analytics` | Productization / Understand | Line-of-sight, viewshed, coverage, route deconfliction | Medium | Implemented and tested |
| `gungnir-policy` | Productization / Assess-Decide | Geofence and readiness policy, chain | Critical | Implemented and tested; wired into the desktop tick (GAP-038), not the node |
| `gungnir-command` | Productization / Assess-Decide | Human approval workflow with a timed queue | Critical | Implemented and tested; wired into the desktop tick (GAP-038, GAP-034, GAP-035), not the node. An expiry is `OperatorDecision::Expired` and a rejection carries its reason, per DN-10 §3 |
| `gungnir-assessment` | Productization / Assess-Decide | Threat/risk scoring, reward matrix | Medium | Implemented and tested; not yet wired |
| `gungnir-decision` | Productization / Assess-Decide | COA rationale; alternatives and what-if | Medium | Rationale implemented and tested; rest trait surface |
| `gungnir-modelops` | Productization / Assess-Decide | Algorithm config governance | Medium | Implemented and tested |
| `gungnir-security` | Productization / Secure-Operate | Roles, authorization, audit | Critical | Authorizer and audit implemented and tested; authentication trait only |
| `gungnir-api` | Productization / Secure-Operate | v1 external contract | High | Types and ICD; transport pending |
| `gungnir-observability` | Productization / Secure-Operate | Health snapshot, alert correlation, watchdog | High | Implemented and tested; watchdog wired in the node |
| `gungnir-resilience` | Productization / Secure-Operate | Store-and-forward, checkpoints, reconciliation | Medium | Implemented and tested |
| `gungnir-collab` | Productization / Secure-Operate | Shared picture sync, authority arbitration | Medium | Implemented and tested |
| `gungnir-workflow` | Productization / Secure-Operate | Role workspaces, alert lifecycle, cases | Medium | Implemented and tested |
| `gungnir-replay` | Productization / Validate | Deterministic session playback | Lower | Implemented and tested |
| `gungnir-reporting` | Productization / Validate | Mission reports and export | Lower | Implemented and tested |
| `gungnir-data` | 3D data ecosystem | Point cloud/VTK/terrain/glTF I/O | Core | Loader surface; background loader thread |
| `gungnir-data-fusion` | 3D data ecosystem | GPU point-cloud registration | Core | CPU reference built and tested: point-to-point ICP (Kabsch), plus per-point normal estimation since 2026-09-07. Point-to-plane and the GPU pipeline are pending (GAP-024); the GPU path also waits on the self-hosted runner (GAP-061) |
| `gungnir-remote` | Deployment | Remote backends over `gungnir-api` | High | Store-and-forward implemented and tested; transport pending |
| `gungnir-node` | Deployment | Headless service-node binary, Linux container | High | Runnable; journals and reports health; no network endpoints yet |
| `gungnir-render` | UI/rendering | Headless wgpu compute device | Core | `GpuContext::new` implemented; egui-over-wgpu path inactive |
| `gungnir-viewport3d` | UI/rendering | Camera, glyphs, 2D fallback, tiles/VTK bridges | Core | 2D fallback implemented and tested; three-d scene pending |
| `gungnir-ui` | UI/rendering | 2D dashboard panels, theme | Core | Implemented; wired |
| `gungnir-app` | UI/rendering | Desktop binary, `AppState`, tick | Core | Runnable; backend selection, session, journal, ingest, planning wired. Library target added 2026-09-05 so the tick harness can reach `update::tick`; four desktop budgets measured, one gated, one failing (GAP-085) |

---

## 9. Principal Risks If Remaining Gaps Aren't Addressed

**The current service contracts are too thin to build on top of long-term.** *(Revisited 2026-09-06 under GAP-066: the result types were added; the "security context" this paragraph asks for was struck by the owner, because authentication lives at the gateway, authorization at the point of action, and releasability at the point of release -- none of them in the facade. The paragraph is kept as the record of what was asked.)*
`gungnir-tracking-service`/`gungnir-intercept-service`'s `submit_detection`,
`tracks`, and `plan` communicate the right conceptual boundary, but omit lifecycle,
errors, quality, security context, and provenance that `gungnir-model` (Part 5)
already defines. Building further ingestion adapters or UI tightly against today's
minimal contract risks rework once the migration noted in Part 2's follow-up items
happens.

**`AppState` must not become the system of record.** The single-source-of-truth
rule is correct for a desktop app's *rendering* state, but if it's also treated as
the durable, authoritative record of all tracks, plans, and operator actions, then
persistence, multi-user support, and replay all become expensive retrofits rather
than natural extensions. `gungnir-app::AppState` is deliberately documented (in its
own doc comments, and in `ARCHITECTURE.md` §7.3) as a projection of state that
should live in `gungnir-store`, not the mission state itself; in the connected
profiles the authoritative journal is on the service node (`ARCHITECTURE.md` §8).

**The two "fusion" pipelines can coexist visually while staying statistically
disconnected.** `gungnir-data-fusion`'s GPU point-cloud registration and
`gungnir-track-fusion`'s sensor registration/bias estimation are correctly kept as
separate crates, but nothing currently passes registration output (estimated
transform, uncertainty, calibration version) into the tracking side as evidence.
Without that connection, the 3D view could show a well-aligned point cloud while
the tracker's own bias model knows nothing about it — a plausible-looking but
analytically untrustworthy combined picture.

**Verification proves algorithm correctness, not operational readiness.** The
existing verification stack (Part 1's oracle/property/concurrency/fuzz gates) is
genuinely strong evidence the *math* is right. It is not evidence the *system* is
ready to field — interface conformance, security testing, replay/persistence
integrity, and operator-workflow testing are a different, currently uncovered
claim, and Parts 2–5's crates have only draft verification rows
(`verification-capability-table.md` §2) with pass criteria still to be agreed.
Treating "the tracking core's CI is green" as "ready to deploy" would be a
false-confidence risk worth flagging explicitly to stakeholders — and the CI
workflows, while present (`gungnir-workspace-structure.md`), only run once the
repository is hosted with runners.

**The connected profiles are scaffolded but not yet connectable.** The service-node
binary, the desktop's remote backends, the v1 API contract, store-and-forward, and
reconciliation all exist and are tested in isolation; the API transport does not.
Until it lands, a desktop configured for a remote node falls back to embedded
services with an alert, only the disconnected profile can be exercised end to end,
and the system-of-systems claims in this document should be read as design intent
with working parts rather than a fielded capability.

---

## How to read this alongside the other project documents

- **`ARCHITECTURE.md`** (workspace root) is the technical crate-boundary reference
  this document's business descriptions map onto — read it for *how* the dependency
  graph is wired and *where* each crate runs in the three deployment profiles; read
  this document for *what* each piece does and *why* it matters.
- **`verification-capability-table.md`** and **`scenario-crate-narrative.md`**
  remain the source of truth for Part 1's pass/fail criteria and test-scenario
  rationale. Parts 2–5 have draft rows in that table's §2.
- **`architecture.md`** (in `docs/`) maps every crate to its verification rows;
  **`docs/README.md`** indexes the whole set and carries the glossary.
- This document expects to change as the remaining scoping decisions get made:
  multi-user conflict resolution, disconnected reconciliation, and the remaining
  items in Part 5.6 are requirements questions, not just engineering tasks.
  Deployment topology and real-time action versus recommendation-only are decided:
  three profiles from one crate set, and recommendation-only with a recorded human
  decision before any resource acts.

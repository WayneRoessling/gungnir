# DN-29 A third mode for the pipeline's IMM

Scopes the follow-up `docs/design/DN-28-imm-in-the-pipeline.md` §7 named and deliberately
did not close: "the whole-scenario-1 acceptance question... a third IMM mode needs a
heterogeneous-state-dimension redesign `Imm<N, M>` does not have, which is its own design
note." This is that note. Status: **proposed 2026-09-07; §5's recommendation signed by the
owner 2026-09-09. No code exists.**

This is a scoping document, matching DN-28's own two-signature discipline (a signature on
a design says the design is the right one to build; a signature on an implementation says
the code does what it says): it names the problem, weighs two designs, recommends one, and
states what is explicitly out of scope, so an owner can sign the shape of the work before
anyone writes the `gungnir-core` or `gungnir-filters` diff. **That signature is what is
recorded here** -- the augmented-state design (§5) is the right one to build. It is not a
signature on any code, because none exists yet: **the implementation still needs §6's
three open questions answered or explicitly deferred to its own verification gate before
that future diff is itself signed**, the identical two-step DN-28 went through.

## 1. The gap DN-28 left, stated precisely

DN-28 wired `Imm<6, 3>` over a constant-velocity and coordinated-turn mode into the fusion
pipeline (`FilterSelection::ImmCvCt`) to fix track fragmentation, and scoped itself
deliberately to that pair because both are six-dimensional (§2: "`Imm<6, 3>` over a CV mode
and a CT mode is dimensionally identical to the pipeline's existing... same state vector,
same position-only measurement"). Building its acceptance test
(`gungnir-tracking-service/tests/scenario_truth_replay.rs::the_imm_confirms_one_track_through_the_turn_where_constant_velocity_fragmented`)
found that scenario 1's own comparison instant — the scenario's last observation, per
`plan_maneuvering_aircraft` in `gungnir-scenario/src/lib.rs` — falls in the scenario's
**third** phase, constant acceleration (`until_s: 200.0` closes the coordinated-turn phase;
the scenario runs to `duration_s: 300.0` and the radar in `plan_maneuvering_aircraft`
scans once a second, so the last observation sits deep in the third phase).
`gungnir_core::ConstantAcceleration` implements `MotionModel<9>` — position, velocity,
**and** acceleration per axis — so a six-dimensional CV/CT IMM cannot represent this regime
regardless of tuning. DN-28's row was therefore truncated to source time < 199 s, covering
the scenario only through the end of the coordinated-turn phase, and its §7 named the
remainder as this note's job.

**This is not a tuning gap.** No transition matrix, no process-noise figure, and no gate
threshold changes the fact that a 6-dimensional state vector has no acceleration component
to estimate. The fix is structural: a third mode whose own state is 9-dimensional, in an
`Imm` type whose whole design assumes one shared dimension across every mode.

## 2. The owning components

| Concern | Crate | Trust |
|---|---|---|
| Whichever new motion model(s) this note recommends | `gungnir-core` | Low (numerical stability) |
| Any change to `Imm`/`ModeFilter`'s generic shape | `gungnir-filters` | Low (numerical stability) |
| The third pipeline branch and its settings | `gungnir-fusion-async` | Low (concurrency, out-of-order handling) |
| The `imm-cv-ct-ca` selection and its schema | `gungnir-config` | Medium (mechanical, DN-28 §5's pattern) |
| Verification | `gungnir-core`, `gungnir-filters`, `gungnir-tracking-service` tests | -- |

No dependency edge changes under either candidate design in §4: both stay inside the
`gungnir-core` → `gungnir-filters` → `gungnir-fusion-async` chain DN-28 already used.

## 3. Why `Imm<N, M>` cannot hold a `ConstantAcceleration` mode as it stands

`Imm`'s modes are stored as `Vec<Box<dyn ModeFilter<N, M>>>` (`gungnir-filters/src/imm.rs`),
and `N` is a const generic parameter of `Imm` itself, fixed once at the call site
(`Imm<6, 3>`). `ModeFilter<N, M>` is implemented generically for
`KalmanFilter<Model, N, M> where Model: MotionModel<N>` — so a `KalmanFilter` only
implements `ModeFilter<N, M>` for the one `N` its motion model was built against.
`ConstantAcceleration` implements `MotionModel<9>`, so
`KalmanFilter<ConstantAcceleration, 9, 3>` implements `ModeFilter<9, 3>`, never
`ModeFilter<6, 3>`. `Box::new(ca_filter) as Box<dyn ModeFilter<6, 3>>` does not type-check —
this is a compile-time wall, not a runtime tuning problem, and it holds regardless of how
`Imm::new`'s transition matrix or mode probabilities are chosen.

Two shapes of fix follow from where the mismatch is resolved: pad the two smaller modes up
to nine dimensions so all three genuinely share `N = 9` (§4a), or teach `Imm` to hold modes
of different native dimensions behind a shared reporting dimension (§4b).

## 4. Two candidate designs

### 4a. Augmented-state IMM: pad CV and CT to nine dimensions

Give the constant-velocity and coordinated-turn modes a second, nine-dimensional
implementation each — new types, not modifications to `ConstantVelocity` or
`CoordinatedTurn`, which `CLAUDE.md` forbids redefining and which the pipeline's existing
`FilterSelection::ConstantVelocity` and `ImmCvCt` variants still need at six dimensions.
Call them `ConstantVelocityAugmented` and `CoordinatedTurnAugmented`, using
`ConstantAcceleration`'s own state layout (`[e, n, u, vₑ, vₙ, vᵤ, aₑ, aₙ, aᵤ]`, per
`gungnir-core::ConstantAcceleration::f`) so all three modes agree on what column 6 means:

```rust
// gungnir-core: new types, MotionModel<9>. Sketch, not a decided implementation.
impl MotionModel<9> for ConstantVelocityAugmented {
    fn f(&self, dt: f64) -> SMatrix<f64, 9, 9> {
        let mut f = SMatrix::<f64, 9, 9>::identity();
        for axis in 0..3 {
            f[(axis, 3 + axis)] = dt; // position from velocity, exactly CV's own block
            // no (axis, 6+axis) or (3+axis, 6+axis) term: acceleration does not
            // couple into position or velocity here, which is what "constant
            // velocity" means even in the padded state.
        }
        f
    }
    fn q(&self, dt: f64) -> SMatrix<f64, 9, 9> {
        // top-left 6x6 block: bit-identical to ConstantVelocity::q(dt).
        // bottom-right 3x3 block: a new, small, non-negative phantom term (§6).
        // off-diagonal blocks: zero.
    }
}
```

`CoordinatedTurnAugmented` is the same construction over `CoordinatedTurn::f`'s existing
6×6 rotation block. `Imm<9, 3>` then holds three modes — the two augmented ones and
`KalmanFilter<ConstantAcceleration, 9, 3>` unmodified — with a position-only
`H: SMatrix<f64, 3, 9>` (zero in the acceleration columns, exactly the `H` a standalone CA
filter already uses). **`Imm`, `ModeFilter`, and `KalmanFilter` need no changes at all.**
The pipeline work is additive in the same shape DN-28 already established: a third
`TrackFilter` arm, a third `FilterSelection` variant, and a 3×3 transition matrix /
length-3 probability vector where DN-28 added 2×2 and length-2 ones.

### 4b. Heterogeneous-dimension IMM: redesign `Imm` to hold mixed-dimension modes

Instead of padding the motion models, teach `Imm` itself to hold modes with different
native dimensions `Nᵢ` behind one shared reporting dimension `N`. Concretely: a new wrapper
type, `EmbeddedMode<Inner, const N_OWN: usize, const N: usize>`, holding an inner
`ModeFilter<N_OWN, M>` plus a fixed embedding matrix `E: SMatrix<f64, N, N_OWN>`, and
implementing `ModeFilter<N, M>` by lifting the inner state and covariance through `E` (and
some assigned prior for the directions `E` does not observe) every time `Imm` reads
`state()` or `covariance()`. `Imm<9, 3>`'s mode vector would then mix a
`KalmanFilter<ConstantVelocity, 6, 3>` and a `KalmanFilter<CoordinatedTurn, 6, 3>`, each
behind an `EmbeddedMode<_, 6, 9>`, alongside the native
`KalmanFilter<ConstantAcceleration, 9, 3>`.

This is a real generalization — a future mode at any dimension could join an `Imm` without
a bespoke padded motion model — but it is materially bigger for what this system needs
today, for three reasons. First, it changes public, already-signed, already-gated surface
(`Imm`, `ModeFilter`; item 94's oracle gate and DN-28's `imm.rs` tests) rather than adding
beside it, so every existing guarantee about those types has to be re-argued rather than
inherited. Second, the embedding has to behave correctly inside `predict`'s mixing step,
which reads every mode's `state()`/`covariance()` directly to form the outer-product spread
term (`imm.rs`, `Imm::predict`) — an embedded mode's "covariance" there is not its native
covariance but `E Pᵢ Eᵀ` plus whatever prior is assigned to the unobserved directions,
which is the same quantity §4a's phantom block represents, computed at read time by a
wrapper instead of carried by a motion model. The two designs converge on the same
underlying mathematics for this specific case; 4b just relocates where the augmentation
lives. Third, nothing in this workspace's baselines needs a fourth mode at a third distinct
dimension today, so building the general mechanism now is exactly the kind of unrequested
generality `CLAUDE.md` asks not to design for.

## 5. Recommendation (signed by the owner 2026-09-09: this is the design to build)

**§4a, the augmented-state IMM.** It needs zero changes to `gungnir-filters::Imm` or
`ModeFilter`, both signed off under item 94 and exercised by `imm_diff.rs`'s oracle gate;
it needs zero changes to the `TrackFilter`/`FilterSelection`/`PipelineSettings` shape DN-28
just built and tested, beyond one more enum arm and wider arrays, which is exactly the
one-line-per-touch-point change `pipeline.rs`'s `match` arms are built for (DN-28 §4); and
it needs exactly two new `gungnir-core` types, each smaller than `ConstantAcceleration`
itself, that are additions rather than redefinitions of a type `gungnir-core` owns. §4b is
the more general answer and is disproportionate to the one triple this system needs, for
the same reason DN-28 §2 gave for scoping itself to the CV/CT pair rather than the whole of
item 94's blanket statement: the smaller, concrete case is not the general problem, and
building the general mechanism to solve the concrete case is solving the wrong amount of
problem.

**The owner signed this recommendation on 2026-09-09, on its design merits alone**: no
`gungnir-core`, `gungnir-filters`, or `gungnir-fusion-async` diff exists yet, and this
signature does not stand in for the one that diff will need on its own account (per this
note's own two-signature discipline, restated at the top). §6's three questions are
unanswered as of this signature and remain the gate before any implementation lands.

## 6. The open numerical-stability question §5 still has to answer before sign-off

This note recommends a design; it does not settle its numbers. Three questions, to be
argued from the same oracle-comparison discipline the rest of this table uses rather than
assumed, before any implementation PR:

- **Whether the padding is inert when it should be.** The augmented CV/CT modes' `f` has no
  entry coupling the acceleration block into position or velocity (§4a's sketch), so the
  standard quadratic-form propagation `F P Fᵀ` cannot carry any cross-covariance mixing
  manufactures in the phantom block back into the real position/velocity subspace on the
  next `predict` — the argument is structural, not a hope, but it has not been tested. The
  property to gate: driven by pure CV or CT truth (never favouring the CA mode), the
  position/velocity marginal of a three-mode `Imm<9, 3>` must agree with today's
  `Imm<6, 3>` to the same 1e-4 tolerance `imm_diff.rs` already gates on. A failure here
  means the padding leaks, and the design in §4a does not work as argued.
- **What the phantom block's own process noise should be.** Exactly zero is PSD-valid
  (`gungnir-core::assert_psd` accepts a singular-but-semi-definite covariance, per its own
  `singular_but_psd_is_accepted` test) but leaves the augmented modes' acceleration
  estimate permanently uninformative until mixing pulls in something from the CA mode; too
  large invents information the CV/CT modes should not claim to have. This is a tuning
  question with a clear failure mode to gate on: too small and the augmented modes never
  hand CA a useful lead at a manoeuvre's onset; too large and the augmented modes'
  reported combined covariance overstates what a "no acceleration" belief should claim
  during a straight run.
- **Whether the accel prior at track initiation should differ between the CA mode and the
  two augmented modes.** A track starting under CV truth should not begin with CA's own
  acceleration prior on a state variable CV asserts is exactly zero-coupled; this is the
  IMM's ordinary mode-transition/mixing machinery answering a question it already exists to
  answer, but it is a live question for this pair specifically and should be argued rather
  than defaulted.

## 7. Explicitly out of scope

- **No change to `Imm`, `ModeFilter`, or `KalmanFilter`'s public API.** §5's recommendation
  is chosen specifically to avoid this.
- **No fourth mode and no other `MotionModel` pairing** (a nonlinear or bearing-only CA
  variant, for instance). This note is scoped to exactly the CV/CT/CA triple scenario 1
  already exercises, the same discipline DN-28 §2 applied to its own pair.
- **No re-litigation of DN-28 §3's open gating question** (whether the combined-covariance
  gate needs a separate, wider threshold during mode disagreement). A third mode makes
  disagreement more frequent, not different in kind; the question carries over unanswered
  rather than being decided here.
- **No fix for the `measurement_noise_var` mismatch DN-28 §7 flagged** (the pipeline
  default `[400, 400, 900]` against scenario 1's actual sensor variance
  `[625, 3600, 22500]`). Orthogonal to the dimension problem and already named as its own
  follow-up.
- **No claim that scenario 1 is tracked through its full 300 s duration.** That is the
  acceptance test §8 proposes building, once this note is signed and implemented — not a
  result this note reports.

## 8. Verification plan (proposed; not built)

Per this directory's own rule that a criterion is agreed before the code exists, three new
rows, none of them a widened version of an existing one (`CLAUDE.md`'s rule against
widening a pass criterion to make a test pass):

| Component | Test | Method | Criterion |
|---|---|---|---|
| `gungnir-core` augmented motion models | `gungnir-core/tests/motion_models_diff.rs` (extended) | Neither `filterpy` nor Stone Soup has a notion of a padded motion model, so this is a structural check rather than an oracle comparison: each augmented model's top-left 6×6 `f`/`q` block is bit-identical to the unaugmented model's own, at every `dt` the existing row already tests; the acceleration block of `f` is exactly the identity | Bit-identical top-left block; identity acceleration block in `f` |
| `gungnir-filters` padding inertness | `gungnir-filters/tests/imm_diff.rs` (extended) | The property in §6's first bullet: pure CV/CT truth driven through the three-mode `Imm<9, 3>`, position/velocity marginal compared against today's `Imm<6, 3>` | Relative error < 1e-4, the existing IMM row's own tolerance |
| `gungnir-tracking-service` scenario 1, full duration | `gungnir-tracking-service/tests/scenario_truth_replay.rs` (new test, alongside DN-28's truncated one) | Scenario 1 scored at the true comparison instant (no truncation), through all three phases | Bound to be argued from the scenario's own sensor noise and target separation once the estimator exists to measure, per this file's existing practice (§ module documentation, "The bounds, and why each one") |

## 9. Sizing and what a sign-off has to cover

Medium, larger than DN-28 itself: DN-28 could reuse the whole of `Imm` unchanged, and §5's
recommendation still adds two new `gungnir-core` motion models plus a third pipeline
branch across `TrackFilter`, `FilterSelection`, `PipelineSettings`, and `gungnir-config`'s
validation — each following a pattern DN-28 already established rather than inventing one,
but three touch points rather than DN-28's two. Touches `gungnir-core` (low trust:
numerical stability) and `gungnir-fusion-async` (low trust: concurrency, out-of-order
handling) exactly as DN-28 did, plus `gungnir-filters` only if §6's investigation finds the
inertness property does not hold as argued and some (still API-compatible) adjustment to
`Imm`'s combination step is needed.

Per `docs/agentic-workflow.md`, this was a design note only, and the precondition its own
opening paragraph named — an owner's signature on §5's recommendation — is now met
(2026-09-09). An agent may now draft the `gungnir-core`/`gungnir-filters`/
`gungnir-fusion-async` diff on that basis, but the mandatory verification gate still has
to include §6's three questions answered — either here in a signed amendment or in the
implementation PR's own written argument — before the code is `main`-worthy, per the same
low-trust-tier reasoning DN-28 §8 stated for itself. **Not requested this session**: the
signature above covers the design only, and no implementation PR was asked for.

## Traceability

Motivated directly by `docs/design/DN-28-imm-in-the-pipeline.md` §7, which names this note.
`ARCHITECTURE.md` §10 item 94 is the origin of the "different state dimensions... its own
increment" language both this note and DN-28 quote. GAP-011 (Area A) is the register entry
under which DN-28's own motivation sits; this note does not yet have a register entry of
its own — `docs/mission/gap-analysis/gap-register.md` is generated by
`docs/mission/gap-analysis/tools/gen_gaps.py` and is not hand-edited here, so a gap number
for the third-mode follow-up, if wanted, is that tool's output rather than this note's
business. Related: DN-24 (mission profiles and algorithm baselines), whose §7 rule that a
baseline's settings are what the pipeline is actually built with is why §4a's new modes are
selected through `PipelineSettings`/`ConfigBaseline` rather than any other path.

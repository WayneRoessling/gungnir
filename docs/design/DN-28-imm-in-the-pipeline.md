# DN-28 Selecting the IMM in the fusion pipeline

Closes the gap `ARCHITECTURE.md` §10 item 94 named but did not build: "selecting between
filters per mission profile needs the pipeline to hold more than one filter type... that
is its own increment." Status: **proposed 2026-09-07, signed by the owner 2026-09-07.
Built and gated 2026-09-07, and signed by the owner the same day.**

**What each signature settled.** The design signature is on the design -- the scope in
§6, the gating question raised as open in §3, and the sizing in §8. The code signature is
on the `gungnir-fusion-async`/`gungnir-filters`/`gungnir-config` diff built from it (§4,
§5): the `TrackFilter` enum and its interleaving with the reorder buffer
(`gungnir-fusion-async`, concurrency correctness), the gating formula on `Imm`'s combined
estimate (`gungnir-filters`, numerical stability), and the config validation mirroring
`Imm::new`'s own rules (`gungnir-config`). Per the distinction this directory's other
notes draw throughout (DN-16, DN-18, DN-27): a signature on an implementation says the
code does what it says; a signature on a note says the design is the right one to build.
§3's gating question -- whether gating against the IMM's combined, spread-widened
covariance is the right choice -- is answered by §7 item 2's clean isolation (identical
settings, only `filter_selection` differs, `kf-cv` still does not confirm through the
turn and `imm-cv-ct` does) rather than by argument alone, and that evidence is what the
code signature is on.

Motivated by a defect, not a feature request: the track-fragmentation finding recorded on
GAP-011 (scenario 1's single manoeuvring aircraft produces three tracks, none confirmed;
scenario 2's six vessels produce ten). Diagnosed 2026-09-07 alongside the
`TrackView::mission_time` fix (session note, not yet a register entry): raising
`process_noise_psd` 1000x over default reduced scenario 1 from three tracks to two, never
zero. The fixed constant-velocity filter cannot follow the coordinated turn and the
acceleration phase truth's own generator puts it through, and no amount of process-noise
tuning substitutes for a model that can represent a turn. This is not a parameter to widen;
`gungnir-filters` already has the IMM (`Imm<N, M>`, item 94, gated against `filterpy`
1.4.5 within 1e-4 state / 1e-3 mode probability) and the default baseline already names
`imm-cv-ct` — `PipelineSettings::from_baseline` refuses it today because
`IMPLEMENTED_FILTERS` has one entry.

## 1. Owning components

| Concern | Crate | Trust |
|---|---|---|
| The pipeline's per-track filter type | `gungnir-fusion-async` | Low (concurrency, out-of-order handling) |
| The IMM's combined-state gating surface (new) | `gungnir-filters` | Low (numerical stability) |
| The `imm-cv-ct` selection and its settings | `gungnir-config` | Medium (mechanical: schema + validation) |
| Verification | `gungnir-tracking-service`, `gungnir-fusion-async` tests | -- |

No dependency edge changes. `gungnir-fusion-async` already depends on `gungnir-filters`
and constructs `KalmanFilter` there today (`pipeline.rs`); this adds a second type from a
crate already on the graph.

## 2. Why this is smaller than item 94's blanket statement suggests

Item 94 says the filters "have different state dimensions and different update
signatures" as the reason extending `IMPLEMENTED_FILTERS` is not a one-line change. That
is true across the *whole* set (EKF/UKF take a nonlinear range-azimuth-elevation
measurement, the particle filter's state is a cloud, not a Gaussian) but it is **not**
true of the specific pair the default baseline names. `gungnir_core::CoordinatedTurn`
implements `MotionModel<6>` — the same six-dimensional state
(`[e, n, u, ve, vn, vu]`) as `ConstantVelocity` — and turn rate `omega` is a fixed model
parameter, not an estimated state component (`gungnir-core/src/lib.rs`, `CoordinatedTurn`).
So `Imm<6, 3>` over a CV mode and a CT mode is dimensionally identical to the pipeline's
existing `TrackFilter = KalmanFilter<ConstantVelocity, 6, 3>`: same state vector, same
position-only measurement. **This increment is scoped to exactly that pair.** EKF/UKF,
particle, square-root, and JPDA/MHT selection stay out of scope and each remains its own
future row (§6).

## 3. The concrete gap: gating needs a combined innovation the IMM does not expose

`FusionPipeline::associate` gates every live track against every detection in a scan
*before* committing an update, using two inherent methods `KalmanFilter` has and `Imm`
does not:

```rust
// gungnir-filters/src/kalman.rs, used directly in pipeline.rs::associate
pub fn innovation_covariance(&self) -> SMatrix<f64, M, M>
pub fn innovation(&self, z: &SVector<f64, M>) -> SVector<f64, M>
```

`Imm::state()` and `Imm::covariance()` exist (the moment-matched combination, item 94's
own documentation: "not just the average of their covariances" -- it carries the
inter-mode spread). Gating needs the equivalent innovation pair computed from that
combined `(x, P)` against the fixed measurement matrix `H` and noise `R` the pipeline
already builds per track (`FusionPipeline::new_filter`). **This is new `gungnir-filters`
surface, not a workaround in `gungnir-fusion-async`**: it is the same computation
`KalmanFilter::innovation_covariance` does, applied to the IMM's combined state, and it
belongs beside the type whose invariant it depends on.

**A live design question for the reviewer, not a decided answer**: gating against the
combined covariance (which inflates during a mode disagreement, per item 94's own
documentation) will admit a wider association window exactly during a manoeuvre, which is
probably the right behaviour and is also a change in what "the gate" means for a track
running this filter versus one running the plain linear KF. Whether that needs a
separate, wider default `gate_threshold` for IMM-selected tracks or is fine as-is should be
argued from the same oracle-comparison discipline the rest of this table uses, not assumed.

## 4. `FusionPipeline`'s filter storage

`TrackFilter` is a type alias today (`pipeline.rs`):

```rust
type TrackFilter = KalmanFilter<ConstantVelocity, 6, 3>;
```

Becomes an enum over exactly the two selections this increment supports:

```rust
enum TrackFilter {
    ConstantVelocity(KalmanFilter<ConstantVelocity, 6, 3>),
    ImmCvCt(Imm<6, 3>),
}
```

with a small inherent method set (`predict`, `update`, `state`, `covariance`,
`innovation`, `innovation_covariance`) that matches each variant to its underlying call —
not a new public trait in `gungnir-filters`, because nothing outside this one enum needs
to be generic over filter type. `FusionPipeline::new_filter` becomes a `match` on
`self.settings.filter_selection` (a new field, §5) at track-initiation time, which is also
where `IMMEstimator`-equivalent construction (mode filters, initial mode probabilities,
transition matrix) happens per DN-24 §7's rule: a baseline's settings are what the
pipeline is actually built with, not read separately.

**One selection per pipeline instance, not per track.** DN-24 §7 already ties a baseline
to a mission profile and a whole session builds one `PipelineSettings`; nothing in this
scope needs per-track filter choice, and inventing it would be unrequested generality.

## 5. Settings and config additions

`PipelineSettings` gains what `Imm::new` needs and `ConstantVelocity`/`CoordinatedTurn`
do not already supply:

```rust
pub filter_selection: FilterSelection,       // replaces the implicit CV-only today
pub imm_turn_rate_rad_s: f64,                // CT mode's fixed omega
pub imm_mode_transition: [[f64; 2]; 2],      // row-major, DN convention per Imm::new's docs
pub imm_initial_mode_probabilities: [f64; 2],
```

`PipelineSettings::from_baseline` adds `"imm-cv-ct"` to `IMPLEMENTED_FILTERS` and builds
these from new `TrackingProfileConfig` fields (`gungnir-config`, DN-24 §4), which need
their own validation rules matching `Imm::new`'s refusals (§4 above): a transition matrix
whose rows do not sum to one, or initial probabilities that do not, must be refused at
config load, not surfaced as a runtime `FilterError` the pipeline has to recover from.
**This is `gungnir-config` schema work, medium trust, mechanical** given DN-24's existing
pattern for `filter_selection`/`gate_threshold` validation (§6 rule 5).

## 6. Explicitly out of scope, and why each stays its own row

- **EKF/UKF selection.** Different measurement model (range/azimuth/elevation, nonlinear)
  from the position-only linear pipeline; wiring one in changes what a detection's
  covariance has to carry through `to_core_detection`, which this increment does not touch.
- **Particle filter selection.** State is a weighted sample set, not a Gaussian; `Track`'s
  `state`/`covariance` fields would need a lossy projection or a second representation.
  Worth its own design note before any code.
- **Square-root/UDU form.** Same interface shape as the linear KF (state, covariance,
  innovation) so plausibly a smaller follow-on than this one, but not needed to fix the
  fragmentation defect and not bundled here to keep this increment reviewable in one pass.
- **JPDA/MHT as the pipeline's association strategy.** A different axis entirely (which
  detections a track competes for), independent of which filter estimates it; today's
  global-nearest-neighbour association is unaffected by this note.
- **Track-to-track fusion interaction.** GAP-013's fuser already treats a `Track` as an
  opaque `(state, covariance)`; an IMM-produced one needs nothing special there, and this
  note makes no claim about that boundary either way.

## 7. Verification plan

Two new gates, following the pattern the `fusion-async` and `scenario_truth_replay` rows
already use (async-vs-batch, and truth-scored), rather than inventing a third method:

1. **Async-vs-batch, IMM selected**
   (`gungnir-fusion-async/tests/oos_convergence.rs::out_of_order_arrival_converges_on_the_offline_batch_with_imm_selected`).
   `run_batch` and the channel-driven `ingest_with` path agree exactly (the same 1e-4
   criterion as the existing out-of-sequence row) with `filter_selection` set to
   `ImmCvCt`. Proves the plumbing, not the estimator -- item 94 already proved the IMM's
   own math.
2. **Truth-scored, scenario 1's turn phase, IMM selected**
   (`gungnir-tracking-service/tests/scenario_truth_replay.rs::the_imm_confirms_one_track_through_the_turn_where_constant_velocity_fragmented`).
   **Built as planned, but not scored the way this section originally said**, and the
   correction is worth keeping on the record rather than editing away. The plan above
   asked for one confirmed track over the *whole* scenario. Building the row found two
   confounds neither one belongs to `imm-cv-ct`:
   - Scenario 1's comparison instant sits in the scenario's own **third** phase,
     constant acceleration -- `gungnir_core::ConstantAcceleration` is `MotionModel<9>`,
     and no six-dimensional filter, `imm-cv-ct` included, can represent a
     nine-dimensional dynamic. This is §2's own dimensional argument working against a
     phase §2 never claimed to cover; the row now replays only through the end of the
     coordinated-turn phase (source time < 199 s), where truth is still
     six-dimensional.
   - `PipelineSettings::default().measurement_noise_var` (`[400, 400, 900]`) is a
     generic figure no scenario's sensor was tuned against. Scenario 1's actual radar
     reports variance `[625, 3600, 22500]` -- twenty-five times the assumed height
     term -- and understating sensor noise makes every gate too tight regardless of
     motion model. Corrected in this one test's own settings only; the finding is
     flagged as its own follow-up rather than changing the default under every other
     gated row in the workspace.

     With both isolated, the row is clean: identical settings, only `filter_selection`
     differs, `kf-cv` still does not confirm through the turn (fragmentation was mostly
     the noise confound; non-confirmation is what `imm-cv-ct` actually fixes) and
     `imm-cv-ct` does. **This is the acceptance criterion, and it held.**

Both rows are new entries in `docs/verification-capability-table.md` §1/§2 keyed to this
note, not a widened criterion on an existing row (`CLAUDE.md`'s rule against that).

**Two follow-ups this note surfaced and does not close**, named rather than absorbed:
the whole-scenario-1 acceptance question (what fixes the constant-acceleration phase --
a third IMM mode needs a heterogeneous-state-dimension redesign `Imm<N, M>` does not
have, which is its own design note), and the `measurement_noise_var` mismatch between
`PipelineSettings::default()` and each scenario's actual sensor model, which predates
this note and likely affects the fragmentation counts recorded for scenarios 2 through 4
as well as scenario 1.

## 8. Sizing and sign-off

Medium-to-large, not XL: the estimator, its oracle gate, and the association/lifecycle
machinery around it are all already built and signed (item 94). The new work is the
`TrackFilter` enum and its dispatch, the gating pair on `Imm` (§3), the config schema
additions (§5), and the two verification rows (§7) -- comparable in scope to GAP-013
(track-to-track fusion) or GAP-014, each closed in the batch that also closed GAP-011,
not to GAP-011 itself.

Touches `gungnir-fusion-async` (low-trust: concurrency correctness, out-of-order
measurement handling) and `gungnir-filters` (low-trust: numerical stability). Per
`docs/agentic-workflow.md`, the mandatory verification gate ran (§7, all rows passing)
and the owner signed the diff 2026-09-07: `TrackFilter`'s interleaving with the reorder
buffer adds no new await point and no new shared state -- the enum is matched on and
mutated exactly where the old concrete type was, under the same `&mut self` the pipeline
already serializes through one task -- and the gating formula in §3 is the right one
because §7 item 2 measured it rather than argued it.

# DN-29 Measurement noise from the baseline

Closes the follow-up DN-28 §7 named and did not fix: "the `measurement_noise_var`
mismatch between `PipelineSettings::default()` and each scenario's actual sensor model,
which predates this note and likely affects the fragmentation counts recorded for
scenarios 2 through 4 as well as scenario 1." Status: **proposed 2026-09-07, built and
gated 2026-09-07. Not yet signed by the owner** -- it touches `gungnir-fusion-async`
(low-trust: `from_baseline`'s signature) and should get the same review DN-28's code
diff got before merge, per `docs/agentic-workflow.md`.

Motivated by the same diagnosed defect DN-28 was, one layer down: DN-28 §7 found that
`PipelineSettings::default().measurement_noise_var` (`[400, 400, 900]`) understated
scenario 1's actual radar noise by up to 25x, and that correcting it for one test alone
turned three fragmented tracks into one *before* the IMM was even selected -- the
motion-model mismatch DN-28 fixed was real, but the noise mismatch was doing most of the
work the fragmentation finding on GAP-011 was first attributed to. DN-28 deliberately
left the mismatch itself unfixed, in exactly one test's own settings, and named it as its
own increment rather than absorbing it. This is that increment.

## 1. Owning components

| Concern | Crate | Trust |
|---|---|---|
| `measurement_noise_var` on the baseline's tracking schema | `gungnir-config` | Medium (mechanical: schema + validation, same shape as DN-28 §5's `imm-cv-ct` fields) |
| `PipelineSettings::from_baseline`'s signature | `gungnir-fusion-async` | Low (the field already exists on `PipelineSettings`; this is a new argument threaded through, not a change to the filter's own math or its concurrency) |
| Wiring in both binaries | `gungnir-app`, `gungnir-node` | Mechanical |
| Verification | `gungnir-config`, `gungnir-tracking-service` tests | -- |

No dependency edge changes. No new type: `measurement_noise_var: [f64; 3]` already
existed on `PipelineSettings` (`gungnir-fusion-async/src/pipeline.rs`); what did not
exist was a way for a baseline to set it to anything but `Self::default()`'s placeholder.

## 2. Why this is a schema field and not a bigger redesign

`gungnir-scenario::sensor::SensorModel`'s noise (`sigma_range_m`, `sigma_cross_m`,
`sigma_height_m`) is in the sensor's line-of-sight frame, rotated into ENU per detection
by the actual bearing to the target. `PipelineSettings::measurement_noise_var` is one
fixed ENU triple applied to every detection regardless of source or bearing. **This note
does not close that gap.** It follows the exact approximation DN-28 §7 already used and
got signed off on for scenario 1's own test -- treating a sensor's
`[sigma_range², sigma_cross², sigma_height²]` as if it were `[east, north, height]`
variance -- because a deployment naming one sensor's noise as its baseline figure is the
same approximation, made once at configuration time instead of once per test. A
per-detection, frame-aware `R` is a real, larger increment (it would need the sensor's
bearing to reach `FusionPipeline::new_filter`, which is a new edge into the pipeline this
note does not draw) and is named here as its own future row rather than folded in.

## 3. Schema and validation

`TrackingConfig` and `TrackingProfileConfig` (`gungnir-config`) each gain:

```rust
#[serde(default = "default_measurement_noise_var")]
pub measurement_noise_var: [f64; 3],
```

`default_measurement_noise_var()` returns `[400.0, 400.0, 900.0]` -- `PipelineSettings::
default()`'s own figure -- so a baseline file written before this field existed
deserializes to exactly the behaviour it already had, rather than a silently different
one. This is the same backward-compatibility shape DN-28 §5 used for the `imm-cv-ct`
fields, and for the same reason: `#[serde(default)]` alone would default to `[0.0; 3]`,
and every filter selection uses this field, unlike the `imm-cv-ct` ones, so a missing
value has to mean "unchanged" rather than "zero."

**Validated unconditionally**, not gated behind `filter_selection == "imm-cv-ct"` the way
DN-28 §5's fields are: every axis must be finite and strictly positive
(`validate_measurement_noise_var`, mirroring `gate_threshold`'s own rule). An axis at or
below zero states no error, which is not a measurement noise a Kalman filter's `R` can be
built from -- and, concretely, `FusionPipeline::initiate` seeds a freshly initiated
track's *prior* covariance from this same array, so a zero here is a zero prior on that
axis, not a stated noise, and the gate's innovation covariance for that axis is singular
until a predict step's process noise inflates it. `gungnir-config`'s own test
(`a_non_positive_measurement_noise_axis_is_refused_regardless_of_filter_selection`) pins
the refusal.

## 4. `PipelineSettings::from_baseline`

Gains one more argument, appended after `imm` per the same reasoning DN-28 §5 gave for
adding `imm` after `filter_selection`: least churn to the two call sites, and this crate
still may not depend on `gungnir-config` to take the field's owning type directly.

```rust
pub fn from_baseline(
    gate_threshold: f64,
    filter_selection: &str,
    imm: &ImmBaselineFields,
    measurement_noise_var: [f64; 3],
) -> Result<Self, UnsupportedFilter>
```

Applied unconditionally (every selection uses it), and only when every axis is finite and
positive -- `gungnir-config` already refuses anything else before a candidate is
promoted, and this does not assume that and silently falls back to
`Self::default()`'s figure otherwise, the same defensive shape `gate_threshold`'s own
check already has.

`gungnir-app/src/state.rs` and `gungnir-node/src/main.rs` both pass
`baseline.config.measurement_noise_var` through at their existing `from_baseline` call
sites; neither gained a new function, because the value needs no assembly the way the
`imm` fields did (`imm_fields()` builds an `ImmBaselineFields` from three separate config
fields -- this is one field, passed straight through).

## 5. What this does not change

- **The default baseline's behaviour.** A deployment that never sets
  `measurement_noise_var` runs exactly as before (§3's default).
- **Any existing pass criterion.** `docs/verification-capability-table.md`'s scenario 1
  and 2 bounds (500 m, 300 m) are unchanged; only the *measured* figures in that row and
  in `scenario_truth_replay.rs`'s own module documentation are corrected, because the
  tests now run with less wrong sensor noise than before, not because the criterion
  moved.
- **Scenario 2's height axis.** `radar_coastal`'s real `sigma_height_m` is `0.0` --
  §3 is exactly why that cannot be carried into this pipeline's `measurement_noise_var`
  as written, so the height axis in `scenario_truth_replay.rs`'s scenario 2 test is left
  at the default's placeholder rather than the real value, named as its own open point
  rather than papered over with an invented substitute.
- **Scenario 3.** `plan_urban_convoy` runs three sensors of different kinds through one
  pipeline with exactly one `measurement_noise_var` for every detection regardless of
  source; no single figure is "the" correct one for it, and none is invented. Its test
  coverage in `no_target_goes_untracked_in_any_scenario` stays on
  `PipelineSettings::default()`, named rather than silently left as it was.

## 6. Verification

No new §1/§2 row: this is a correction to the *inputs* of an existing row
(`tracking-service` Scenario replay scored against ground truth), not a new capability,
so the existing row's Method and Measured columns are updated in place with what changed
and why (§4 above), per this note's own rule against inventing new rows for what is
already covered.

- `gungnir-config`: `a_non_positive_measurement_noise_axis_is_refused_regardless_of_filter_selection`
  (§3) and `measurement_noise_var_is_read_from_the_baseline`
  (`gungnir-tracking-service/src/lib.rs`) pin the schema and the plumbing.
- `gungnir-tracking-service/tests/scenario_truth_replay.rs`: the two accuracy tests and
  the coverage test now run each single-sensor scenario with that scenario's own noise;
  re-measured worst-case error is 169 m (scenario 1, was 238 m) and 83 m (scenario 2, was
  85 m), both still inside their stated bounds. Scenario 1's track/confirm count in the
  coverage test drops from three tracks (none confirmed) to two (still none confirmed):
  most, not all, of the original fragmentation finding was the noise mismatch, which
  matches DN-28 §7's own finding on the turn-phase-only row exactly. Scenario 2's count
  is unchanged at ten tracks for six vessels (its clutter and dropout rates drive that,
  not its measurement noise).

## 7. Sizing and sign-off

Small: no new estimator, no new type, one field threaded through a schema and a function
signature already carrying two other fields the same way (DN-28 §5). Built and gated
2026-09-07. **Not signed.** `docs/agentic-workflow.md`'s low-trust tier names
`gungnir-fusion-async` for concurrency correctness and numerical stability; this change
touches neither (`from_baseline` gains an argument it validates and stores, with no new
await point, no new shared state, and no change to how a filter is predicted or
updated), but the crate is named by the tier itself rather than by what any one change
inside it does, so it is drafted rather than merged unsupervised, per `CLAUDE.md`.

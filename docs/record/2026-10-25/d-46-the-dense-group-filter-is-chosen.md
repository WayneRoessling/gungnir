# D-46: the dense-group filter is chosen in the baseline

D-46, the owner's decision of 2026-09-15, kept the GM-PHD as the dense-group mode's
default filter and made the GM-CPHD selectable "through the `TrackingConfig` vocabulary
GAP-015's closing action already owed". This item records that build. `gungnir-fusion-async`
is human-owned; its standing is in `../../signatures.md`.

## What was missing

GAP-015's dense-group mode had no way in from a baseline at all. `PipelineSettings::dense_group`
was `None` unless code set it, and its own doc comment said a baseline could not reach it,
because `TrackingConfig` had no words for it and a field outside the governance record
would be a picture no record accounts for. So no deployment could turn the mode on, and
choosing between the two filters was a question with no place to put the answer.

## What changed

- **`gungnir-config`.** `TrackingConfig` and `TrackingProfileConfig` gain
  `dense_group: Option<DenseGroupConfig>`:
  - `filter`: `"gm-phd"` (the default when the section names none) or `"gm-cphd"`.
  - `probability_of_detection`, `clutter_density`, `birth_weight`: each optional, each
    keeping the pipeline's figure when absent.
  - An absent section is the mode off, which is what every earlier baseline meant.
  - Validation refuses a filter outside those two, a detection probability outside `(0, 1]`,
    and a clutter density or birth weight that is not finite and positive. It does so on a
    profile candidate and on `tracking` alike, before anything can be promoted. Rule 5 of
    the profile checks moved into its own `validate_candidate` to stay under the length
    limit; its rules are unchanged.
- **`gungnir-fusion-async`** (human-owned).
  - `PipelineSettings::from_baseline` takes the section as a new argument,
    `Option<&DenseGroupBaselineFields>`. It is an argument, not a builder, so no caller can
    apply a baseline and leave its dense-group section behind.
  - A filter it does not run is refused as `BaselineError::UnsupportedDenseGroupFilter`, and
    a figure it cannot run with as `InvalidDenseGroup`. Neither is ever replaced by the
    default, by DN-24 §7's rule.
  - `DenseGroupFilter::from_name` reads the two names.
  - `DenseGroupFilter::Phd`'s doc comment now records D-46's answer and reason, which D-46
    itself asked for.
- **Only the filter and the scene are a baseline's to move.** The three figures are the ones
  `DenseGroupSettings` already said belong to a deployment's sensor. The mixture cap, the
  cardinality bound and the engagement threshold are derived from figures the workspace
  already states (Scenario 4's 200 tracks, `jpda::MAX_DETECTIONS`), and stay derived.
- **Both binaries.** `gungnir-app`'s `algorithm_settings` and `applied_baseline`, and
  `gungnir-node`'s `tracking_service`, pass the section through. So a baseline naming a
  filter the build does not run takes the existing not-applied path (an alert and an
  ungoverned tracker) instead of running something else under the baseline's name.

## How it was tested

- **End to end** (`gungnir-fusion-async/tests/dense_group.rs`). A baseline section naming
  `"gm-cphd"` runs the CPHD over a 14-target raid and reports a most probable count. One
  naming `"gm-phd"` runs the PHD and reports none. No section leaves the mode off.
- **Only four values move.** The three stated figures replace the defaults, the derived
  settings come out identical to `DenseGroupSettings::default()`, and a section that states
  no figure gives exactly the default.
- **Refusals.** An unknown filter, and six bad figures, are each refused by name.
- **`gungnir-config`.** The section parses in its documented shape, names the PHD when it
  names no filter, and reaches the candidate the binaries apply, for both `tracking` and a
  profile candidate. Five bad sections are refused on both.

`cargo test` passes for `gungnir-config`, `gungnir-fusion-async`, `gungnir-tracking-service`
and `gungnir-modelops`, and for `gungnir-app`'s governance suite. Clippy reports nothing in
the changed files.

## What this does not do

- **The default is unchanged.** No shipped baseline names a dense-group section, so no
  deployment runs differently.
- **Turning the mode on by default** still waits on the `spawn_blocking` plumbing
  `agentic-coding-standards.md` §2.2 asks for. A deployment that names the section runs the
  mode inline, at the cost `PipelineSettings::dense_group` records.
- **Nothing projects the estimate into a picture yet.** That is GAP-015's other remaining
  item.

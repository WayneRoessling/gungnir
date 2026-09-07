# DN-08 Policy configuration and plan validity

Closes GAP-052, which finding F-1 retyped to Mission because nothing was designed, and
which returns to Technical now that this note exists. Status: **signed off by the owner
2026-09-05.** Design only; no code exists yet.
**Human-owned and signed**: everything this note configures is a rule about who may do
what. The owner signed it on 2026-09-05, which also unblocks DN-09 and DN-10, both of which
read the schema defined here. A change to it is a change request under phase H.

## 1. The gap and the thread step it blocks

Identification thresholds, weapons control status, authority rules, staleness policy, and
plan validity periods have no place in `ConfigBaseline`. Every one of them is therefore
hard-coded or absent, and plans cannot expire.

This note gates five others. GAP-012 (staleness), GAP-018 (identification thresholds),
DN-09 (weapons control status and authority), DN-10 (expiry and escalation), and GAP-058
(per-class authorization) all read the section defined here. It is designed before them
for that reason.

## 2. The owning component

`gungnir-config` owns the section and its validation. `gungnir-model` owns the types the
section deserializes into, because `gungnir-policy` and `gungnir-command` both consume
them and neither may depend on configuration.

| Crate | Gains |
|---|---|
| `gungnir-model` | `PolicySettings` and the types beneath it |
| `gungnir-config` | `ConfigBaseline.policy: PolicySettings`, its validation, and the validity window |
| `gungnir-policy` | Reads `PolicySettings` instead of hard-coded values |
| `gungnir-command` | Reads the expiry and escalation settings (DN-10) |

`gungnir-policy` depends on `gungnir-model` and `gungnir-geo`; `gungnir-command` depends on
`gungnir-model` and `gungnir-policy`. Putting the types in the model means **no new edge**.

## 3. Types

In `gungnir-model`:

```rust
/// Everything about how this deployment decides, in one versioned place.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PolicySettings {
    pub identification: IdentificationSettings,
    pub staleness: StalenessSettings,
    pub control_status: ControlStatusSettings,
    pub authority: AuthoritySettings,
    pub decisions: DecisionSettings,
    pub validity: Option<ValidityWindow>,
}

/// Per-class confidence needed before the engine may declare, and the margin the
/// leading hypothesis must hold over the runner-up. GAP-018 implements against it.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct IdentificationSettings {
    /// Class name to minimum confidence, 0.0 to 1.0.
    pub thresholds: BTreeMap<String, f64>,
    pub minimum_margin: f64,
    /// Classes a person must confirm however confident the engine is.
    pub operator_confirms: Vec<String>,
}

/// How long a track may go unobserved before it is drawn and treated as stale.
/// Per class, because a loitering surface craft and a cruise missile do not age
/// at the same rate. GAP-012 implements against it.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct StalenessSettings {
    pub default_s: f64,
    pub by_class_s: BTreeMap<String, f64>,
}

/// Weapons control status per effector layer. DN-09 implements against it.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ControlStatusSettings {
    pub by_layer: BTreeMap<EffectorLayer, WeaponsControlStatus>,
}

/// The authority matrix as configuration: which role may accept which decision
/// for which class at which layer. DN-09 and GAP-058 implement against it.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct AuthoritySettings {
    pub rules: Vec<AuthorityRule>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthorityRule {
    pub action: String,
    pub role: String,
    pub layer: Option<EffectorLayer>,
    pub class: Option<String>,
    /// Pre-delegated cases per D-15. Absent means the role decides case by case.
    pub pre_delegated: bool,
}

/// Timeouts and escalation. DN-10 implements against it.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct DecisionSettings {
    pub expiry_s: BTreeMap<EffectorLayer, f64>,
    pub escalate_after_s: BTreeMap<EffectorLayer, f64>,
}

/// When a baseline is valid. A plan produced outside its window is not applied.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ValidityWindow {
    pub valid_from: MissionTime,
    pub valid_until: Option<MissionTime>,
}
```

## 4. Edges

**None.** The section is model types read by policy and command, both of which already
depend on the model.

## 5. Behaviour

**Every setting has an explicit default, and the defaults are the strictest reading.**
That is the rule that keeps a partially configured deployment safe:

| Setting absent | Behaviour |
|---|---|
| Identification threshold for a class | The class is in `operator_confirms`: a person declares it. Never an automatic declaration at a guessed threshold |
| Staleness for a class | `default_s` applies; if that is absent too, validation fails |
| Control status for a layer | `Hold`. An unconfigured layer does not become weapons-free by omission |
| Authority rule for an action | Denied. An action nobody is granted is an action nobody may take |
| Expiry for a layer | No expiry, and the queue says so, rather than a guessed timeout silently discarding a pending decision |
| Validity window | The baseline is always valid |

The asymmetry in the last two rows is deliberate. Silence about authority must deny;
silence about expiry must not discard. Both errors are visible; only the first is unsafe.

`PolicyChain` gains the settings at construction. `GeofencePolicy` is unchanged. A new
`ControlStatusPolicy` and `AuthorityPolicy` are DN-09's content, not this note's.

**Validity.** A baseline outside its window may be read, replayed, and inspected. It may
not be **promoted**, and a plan produced under a baseline that has since expired is marked
superseded rather than silently applied. Expiry never changes a picture retroactively.

## 6. Configuration and interface delta

`ConfigBaseline.policy: PolicySettings`, defaulting to `Default::default()`, plus
`ConfigBaseline.validity: Option<ValidityWindow>`.

Validation rules added:

1. Every threshold and margin is in 0.0 to 1.0.
2. `default_s` is present when `by_class_s` is non-empty, and every value is positive.
3. Every `AuthorityRule.role` names an adopted role and every `action` names a constant in
   `gungnir_security::actions`. An unknown action is an error, because a misspelled action
   grants nothing and looks like a grant.
4. `escalate_after_s` for a layer is less than `expiry_s` for that layer where both exist;
   escalating after expiry is meaningless.
5. `valid_until`, when present, is after `valid_from`.
6. A rule with `pre_delegated: true` names both a layer and a class, per D-15, which
   pre-delegated one specific case and not a general power.

`SUPPORTED_CONFIG_VERSION` stays 1: additive with defaults.

Interface: no change. Policy settings are not published on the snapshot; what a caller may
do is answered by authorization, not by reading the rules.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-14 Configuration editor | The policy section, with validation inline and a visible diff against the promoted baseline, because this is the section where a wrong edit is most costly |
| PN-01 Status strip | Control status per layer, delegations in force, and the baseline validity state |
| PN-17 Commander summary | Delegations in force and the authority rules that produced them |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-5.6 Baselines and plans | Unit tests over the defaults table, plus a promotion test | Every absent setting resolves to the row in the defaults table above; an unknown action or role fails validation; a plan produced under an expired baseline is marked superseded and never applied; escalation earlier than expiry is enforced | Generated baselines; TT-02 replay for the superseded case |

The defaults table is the test fixture. That is the point of writing it as a table.

## 9. Amendment 1 -- **signed by the owner 2026-09-05**

Raised by GAP-052 on 2026-09-05, by implementing this note's §5 and §8 rather than by
reading it. The same sign-off covers the code that conforms to it. Three of the four criteria in §8 were unmet, and the entry had been closed on
§6's schema alone. The behaviour below is what the note already asks for; what is new, and
what needs a signature, is **how** two of them are reached, because §6 said "Interface: no
change" and both of these change one.

**(a) `ConfigStore::apply` takes the time.** §5 says a baseline outside its window may not
be promoted, and `ConfigBaseline::is_promotable_at` was written for exactly that and never
called. Enforcing it needs a clock, and the trait had none. `apply(&mut self, baseline,
now: MissionTime)` takes it as a **required argument** rather than reading one inside the
implementation: promotion is a time-dependent act, and a store with its own clock could
disagree with the session the rest of the decision path runs on. Refusal is
`ConfigError::NotPromotable`, carrying the window, and nothing is written to disk.

This is deliberately *not* part of `validate`. The same baseline is promotable tomorrow
and not today, and calling that "invalid" would be wrong in both directions -- it would
condemn a file that is fine and it would let expiry look like a defect in the document.

**(b) The known action and role names are supplied by the caller.** §6 rule 3 requires
that an authority rule naming an unknown action or role fails validation, and the reason
given there is the right one: *a misspelled action grants nothing and looks like a grant*.
Nothing performed this check. `gungnir_security::actions::ALL` carries a doc-comment saying
it exists "for validating an authority rule at load" and had no callers.

`gungnir-config` may not depend on `gungnir-security` (`ARCHITECTURE.md` §7.1), so the
check is inverted the way DN-22 §4 inverted journal sealing: this crate declares
`KnownVocabulary` and `validate_authority_names`, and the binary -- which can see both
crates -- supplies the lists. No new dependency edge, and the canonical lists stay the only
lists.

The vocabulary is a **required** argument to `FileConfigStore::new`, not an option with a
permissive default. A store that could be built without one would let a caller skip the
only check that catches this class of mistake, and skipping it looks exactly like passing
it. An empty vocabulary is refused rather than accepting every name, for the same reason.

**(c) Supersession is a third outcome, not a verdict.** §5 says a plan produced under an
expired baseline is marked superseded rather than silently applied. It is now checked
**before** the policy chain runs, and the desktop's `decisions::submit` returns
`Submitted::{Evaluated, Superseded}`. A `PolicyVerdict` could not carry it: reporting a
superseded plan as `Denied` would put a refusal nobody made into the record, and
`RequiresHumanApproval` would leave an item waiting that is never going to be applied.
`InterceptEvent::PlanSuperseded` -- declared in the model since this note landed and never
published -- carries it to the journal.

**(d) PN-01 draws the validity state, in four states rather than two.** §7 asks for it.
`NoWindowConfigured`, `InForce`, `NotYet` and `Expired` are kept apart because an
unconfigured window is not the same claim as a valid one, and "not yet" and "expired" have
different fixes. Nothing is drawn in the ordinary case: an element that said "baseline
valid" every frame would be read past, and the case that matters is the one where the
approval queue has quietly stopped filling and looks exactly like a quiet sector.

Still open after this amendment: §7's PN-17 row, and the TT-02 replay §8 names as the data
source for the supersession case, which needs GAP-046.

## Traceability

GAP-052 (retyped, plan 11 finding F-1); CAP-5.6, CAP-3.6; D-05, D-15;
`../mission/roles-and-stakeholders.md` §4 for the authority matrix;
`../ux/wireframes/WF-14-config-editor.puml`; principles AP-01, AP-02, AP-17.
Read by DN-09, DN-10, and gaps GAP-012, GAP-018, GAP-058.

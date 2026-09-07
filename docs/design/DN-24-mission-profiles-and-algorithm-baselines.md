# DN-24 Mission profiles and algorithm baselines

Unblocks GAP-053. Status: **signed by the owner 2026-09-05**, and implemented the same day
under GAP-086. The sign-off covers the code that conforms to this note, and a second
sign-off the same day covers the three corrections in §5, §6 and §8 that building it
raised — the missed dependency edge, the contradiction between §6 and §9 about
`model.promote`, and the two event variants §8 did not name.

## 1. The gap and the thread step it blocks

CAP-5.7 states that "which filter, association, and learned-model configuration is in force
shall be a validated, promoted baseline **per mission profile**, with rollback".
`gungnir-modelops` implements exactly that state machine — `Candidate`, `Validated`,
`Promoted`, `RolledBack`, with rollback to the previously promoted baseline — and
**nothing in the workspace imports the crate**. Its `ModelBaseline` is keyed on a
`mission_profile: String` and that string appears nowhere outside the crate and the
documents describing it.

GAP-053 was examined on 2026-09-05 and deliberately not wired, for three reasons recorded
in `ARCHITECTURE.md` §10 item 73. This note removes the second and third. The first —
`PIPELINE_IMPLEMENTED` is false, so a promoted configuration cannot yet reach the picture —
is GAP-011 and stays open.

**The reason wiring the registry today would be worse than leaving it alone.**
`ConfigBaseline` carries one `TrackingConfig` and no profile. A registry built from it would
hold exactly one candidate, validate it, promote it, and report a promoted baseline in
force. Every part of that would be true and the whole of it would be theatre: there is
nothing to choose between, nothing a rollback could restore, and a reader of the audit trail
would see governance where none is possible. MT-09's after-action review would be told which
configuration produced a session's tracks by a mechanism that could only ever give one
answer.

## 2. The owning components

| Concern | Crate |
|---|---|
| The profile and baseline identifiers | `gungnir-model` |
| Declaring profiles and their candidate configurations | `gungnir-config` |
| The promotion state machine and rollback | `gungnir-modelops` (exists) |
| Constructing the registry and journaling what is promoted | `gungnir-app`, `gungnir-node` |

## 3. What a mission profile is

**A named operating context in which one algorithm configuration is in force.** Not an
enumeration this note fixes: a sector defending a harbour against small UAS and one covering
an approach corridor against fast movers want different gates, and no list written here would
survive contact with a deployment. A profile is declared in the baseline by name, the way an
endpoint is, and referenced by name from the candidates that belong to it.

The word is the mission layer's already — CAP-5.7's statement and
`gungnir-capabilities.md` §5.4 both use it — so this note gives it a schema, not a meaning.

## 4. Types

In `gungnir-model`:

```rust
/// A named operating context. One algorithm configuration is in force per profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MissionProfile(pub String);

/// Which candidate, in which profile. What `Provenance::algorithm_version` should
/// eventually carry, and what a rollback names.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AlgorithmBaselineId {
    pub profile: MissionProfile,
    pub name: String,
}
```

`ModelBaseline.mission_profile` becomes `MissionProfile` rather than a bare `String`, and
gains the `name` that distinguishes two candidates in one profile. It has none today, which
is the second reason a single-candidate registry could not be governed: two candidates would
have been indistinguishable in the record.

In `gungnir-config`:

```rust
/// One candidate algorithm configuration.
pub struct TrackingProfileConfig {
    /// Names an entry in `mission_profiles`.
    pub profile: String,
    /// Unique within the profile; this is what a promotion and a rollback name.
    pub name: String,
    pub filter_selection: String,
    pub gate_threshold: f64,
    /// **The one candidate per profile that starts in force.** Exactly one, see §6.
    #[serde(default)]
    pub promoted: bool,
    /// What validated it, for the review that asks. Free text: this build has no
    /// evidence store, and a structured field nothing populates would be worse.
    #[serde(default)]
    pub validated_by: Option<String>,
}
```

`ConfigBaseline` gains `mission_profiles: Vec<String>`, `tracking_profiles:
Vec<TrackingProfileConfig>`, and `active_profile: Option<String>`.

## 5. Edges

`gungnir-app` → `gungnir-modelops` and `gungnir-node` → `gungnir-modelops`. Both are
downward from a binary, which `ARCHITECTURE.md` §7.1 already describes as depending on
everything above it, and both are drawn in its edge table in the change that adds them.
`gungnir-modelops` → `gungnir-config` already exists and is already drawn.

**Correction, 2026-09-05 (GAP-086), signed by the owner 2026-09-05.** This section
missed one:
`gungnir-modelops` → `gungnir-model`. The crate had no edge to the model — it is one of the
four the §7.1 graph names as not using it — and §4 puts `MissionProfile` and
`AlgorithmBaselineId` there, so the registry cannot be keyed on them without it. The types
have to be in `gungnir-model` regardless: `Provenance` lives there and §7 has it carrying an
`AlgorithmBaselineId` once the pipeline applies one. The edge is downward from a crate that
already depends on `gungnir-config`, which depends on `gungnir-model`, so the graph stays
acyclic. The alternative — keying the registry on two bare strings and building the identity
in the callers — would put a second representation of what a baseline *is* inside the crate
that owns baselines, which is the "two answers" failure this note refuses everywhere else.

Beyond that correction, no other edges. In particular **`gungnir-tracking-service` does not
gain one**: it is told its configuration as data, not handed a crate to ask (see §7).

## 6. Behaviour

**The baseline declares the starting position; the registry governs the session.** A
deployment's file says which candidates exist and which one is in force when the console
opens. Promotion and rollback at runtime change what is in force for that session and are
journaled; they do not rewrite the file. Persisting a runtime promotion into the baseline is
a separate, already-governed act: editing the file and applying it through PN-14, which
requires `config.apply` and leaves an audit entry.

**Validation, and the reason for each rule:**

1. Every `profile` a candidate names must appear in `mission_profiles`. Same rule as
   endpoints, same reason: a candidate in a profile nobody declared is a configuration that
   can never be selected and reads like one that can.
2. **Exactly one candidate per declared profile carries `promoted: true`.** Zero means
   nothing is in force and the deployment cannot say what is running; two means it cannot
   say either, and would report whichever the iteration order happened to reach.
3. `active_profile`, when present, names a declared profile. When absent, and exactly one
   profile is declared, that one is active; when absent and several are declared, the
   baseline is refused rather than a profile being picked.
4. Candidate `name` is unique within its profile. A rollback names a candidate, and two
   candidates with one name make the record ambiguous about what was restored.
5. Each candidate's `gate_threshold` is finite and positive and its `filter_selection` is
   non-empty — the two rules `validate` already applies to `tracking`, applied per
   candidate instead.
6. **`tracking` and `tracking_profiles` are mutually exclusive.** Both present is two
   answers to what is in force.

**Compatibility, so no existing baseline breaks.** `SUPPORTED_CONFIG_VERSION` stays 1: this
is additive with defaults. A baseline carrying `tracking` and no profiles is read as a
single implicit profile named `default` with that configuration promoted — which is exactly
what such a deployment means today, stated in the new vocabulary rather than reinterpreted.
A baseline carrying neither has no algorithm configuration in force, which is the state of
every default deployment and is reported as such rather than defaulted to a guess.

**Promotion is an authorized act.** `gungnir_security::actions::PROMOTE_MODEL`
(`"model.promote"`) has existed since the roles landed. It is in the authorization table —
`role_permits` grants it to the analyst — and **nothing has ever checked it**, because
nothing promotes.

**Correction, 2026-09-05 (GAP-086), signed by the owner 2026-09-05.** This paragraph and
§9 could not both be
true, and §9 is the one that holds. It defers the panel that would make a runtime promotion
reachable, so **nothing in this increment promotes at runtime and nothing checks
`model.promote`**. Promotion here is what the baseline declares, and changing it is
`config.apply` — authorized, audited, and already built. Adding an authority check no caller
can reach would be the "implemented but unwired" pattern this register keeps finding, one
layer deeper. The action gets its first checker with the promote control, and that is a
separate increment.

**The registry re-runs its gate at load.** `InMemoryModelRegistry::validate` is structural
today. A candidate marked `promoted` in the file is registered, validated, and promoted
through the real state machine at startup, so `promote` still refuses anything not
`Validated` and the invariant is not bypassed by the file asserting it. A promoted candidate
that fails the gate refuses the baseline.

**Rollback restores the previously promoted candidate for the profile**, which is the
existing behaviour and the reason more than one candidate has to exist for the feature to
mean anything.

## 7. What this note deliberately does not do

**It does not let a track claim a governed configuration produced it.** `Provenance::
algorithm_version` is documented as the version of the configuration that produced the
track. `gungnir_fusion_async::ingest` ignores any configuration it is given, so until
GAP-011 lands the honest stamp is the one GAP-053 already put there:
`UNGOVERNED_ALGORITHM_VERSION`, which names what is missing. **The rule for whoever lands
GAP-011: the tracking service may stamp an `AlgorithmBaselineId` only once it applies one.**
A stamp that ran ahead of the pipeline would put a governed-looking version on every track
in the journal, and nothing downstream could tell.

So after this note GAP-053 is still open, and blocked on exactly one thing instead of three.

## 8. Configuration and interface delta

`ConfigBaseline` gains `mission_profiles`, `tracking_profiles` and `active_profile` as
above; `tracking` is retained and mutually exclusive with them.

`Event` gains `Governance(GovernanceEvent)` with `Promoted { baseline: AlgorithmBaselineId,
by: String }`, `RolledBack { profile: MissionProfile, restored: Option<AlgorithmBaselineId>
}`, and `PromotionRefused { baseline: AlgorithmBaselineId, reason: String }`. The third
exists for the same reason a denied plan is recorded: a refusal nobody can see is a refusal
that will be argued about later.

**Correction, 2026-09-05 (GAP-086), signed by the owner 2026-09-05.** Two more variants,
both because the
alternative was to say something untrue at session start. `InForceAtStart { baseline }` is
not a `Promoted`: at startup the file said what is in force, nobody promoted anything, and
`by` would have had to be fabricated — nobody is signed in while a session is being built.
`NoneInForce { profile }` is not silence: a deployment that declares no algorithm
configuration governs nothing, and **"nothing was in force" and "we did not record it" are
opposite claims** to whoever reads the journal afterwards. Both are published once per
session, not once per frame.

Interface: no change. Which configuration is in force is not published on the snapshot in
this increment; a peer asking what another node is running is a coalition question and
belongs with DN-18.

## 9. User-interface delta

| Panel | Change |
|---|---|
| PN-14 Configuration editor | The profiles section: each declared profile, its candidates, which is promoted, and what validated it. Read-only in this increment, because promoting from a panel needs the authority check and a confirmation surface that PN-07 provides for plans and nothing provides for this |
| PN-01 Status strip | The active profile, and **only when more than one is declared**. A deployment with one profile gains nothing from being told which it is, and an element that is always the same trains an operator past it |

**No governance panel is proposed.** `docs/ux/ux-to-code-map.md` §1 has twenty panels and
none of them is a model-governance surface; adding a twenty-first is a UX decision, not a
design-note one, and GAP-055 tracks the sixteen already designed and unbuilt.

## 10. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-5.7 Govern algorithm and model baselines | Unit tests over the validation rules, plus a promote-and-roll-back test through the registry the binaries construct | A profile with no promoted candidate, with two, or naming an undeclared profile is refused; a baseline with both `tracking` and `tracking_profiles` is refused; a baseline with only `tracking` yields one implicit `default` profile with that configuration promoted; a rollback restores the previously promoted candidate by name and journals it; a promotion without `model.promote` is refused and the refusal is journaled | Generated baselines |

The criterion that matters most is the second-to-last: **rollback restoring a candidate by
name is the one thing a single-candidate registry could never demonstrate**, and it is why
this note exists.

## Traceability

GAP-053, and a new register entry for the schema work; CAP-5.7; MT-09;
`../ml/mlops.md` §1, whose GAP-078 extends `ModelBaseline` with a model manifest and reads
this note's identifiers; `../gungnir-capabilities.md` §5.4;
`../mission/capabilities/capability-statements.md` CAP-5.7;
`../ux/wireframes/WF-14-config-editor.puml`; principles AP-02, AP-08.
Blocked for effect on GAP-011, which is Area A.

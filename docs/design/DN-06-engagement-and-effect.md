# DN-06 Engagement tracking and effect assessment

Closes GAP-043. Status: first draft, 2026-09-05. **Design only; no code exists.**

## 1. The gap and the thread step it blocks

After a decision, nothing links the engaged track to an outcome. `InterceptEvent` has
`PlanProposed`, `PlanApproved`, and `PlanSuperseded`, and stops there. So MT-01 step 8
(assess the effect) has no data, re-engagement is a judgement made from the map, and
MOE-01 cannot be computed after the fact.

## 2. The owning component

`gungnir-intercept-service`, the facade that already owns the plan lifecycle.

This is the one closure in the set that **cannot** take a dependency edge under any
justification. Engagement state is keyed by the decision, and decisions live in
`gungnir-command`, a productization crate. `gungnir-intercept-service` is a service facade,
and a facade may not depend on productization (AP-10). The design therefore keys on the
`PlanId` the canonical model already owns and on a decision identifier the model gains,
which is the correct answer rather than a workaround: the facade should not know how
approvals are stored.

## 3. Types

In `gungnir-model`:

```rust
/// Identifies one recorded decision. `gungnir-command` mints it; everything else
/// refers to it. Introduced so engagement state can key on a decision without
/// anyone depending on the crate that records decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
         serde::Serialize, serde::Deserialize)]
pub struct DecisionId(pub u64);
```

In `gungnir-intercept-service`:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Engagement {
    pub decision: DecisionId,
    pub plan: PlanId,
    pub track: TrackId,
    pub resource: ResourceId,
    pub started: MissionTime,
    pub state: EngagementState,
    pub history: Vec<EngagementTransition>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EngagementState {
    /// Decided and handed off; nothing observed yet.
    Committed,
    /// The effector reported it acted (DN-07 delivers this).
    Executing,
    /// The engaged track ended in a way consistent with success.
    Effective { evidence: EffectEvidence },
    /// The track persisted past the window in which an effect was expected.
    Ineffective { evidence: EffectEvidence },
    /// The engagement was abandoned before an effect could be judged.
    Aborted { reason: String },
    /// The window closed and the evidence does not support either conclusion.
    Indeterminate { reason: String },
}

/// What the conclusion rests on. Always present, so no outcome is an assertion.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EffectEvidence {
    pub source: EffectSource,
    pub observed_at: MissionTime,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EffectSource {
    /// Inferred from the track's own lifecycle: deletion, coasting, a change of
    /// behaviour. Weak evidence and labelled as such.
    TrackLifecycle,
    /// Reported by the effector system through the handoff channel (DN-07).
    EffectorReport,
    /// A person judged it.
    OperatorAssessment,
}
```

## 4. Edges

**None, and one deliberately refused.** See section 2.

## 5. Behaviour

An engagement opens when a decision accepts a plan, and its identifier is carried on the
`CommandEvent::Decided` event, which gains a `decision: DecisionId` field.

Transitions:

| From | To | Trigger |
|---|---|---|
| `Committed` | `Executing` | An effector report arrives (DN-07) |
| `Committed` or `Executing` | `Effective` | The engaged track is deleted inside the expected window, or an effector reports success |
| `Committed` or `Executing` | `Ineffective` | The track persists past the window, or an effector reports failure |
| Any open state | `Aborted` | The plan is superseded, or a person aborts with a reason |
| Any open state | `Indeterminate` | The window closes with neither condition met |

**`Indeterminate` is the point of this design.** Without it, an engagement whose outcome
nobody observed becomes either a false success or a false failure, and MOE-01 computed
from those numbers is worse than no number at all. The state exists so that "we do not
know" is a first-class result, which is AP-02 applied to a measure.

**The weakness of track-lifecycle evidence is stated, not hidden.** A track deleted inside
the window may have been destroyed, may have flown behind terrain, or may have been
dropped by the tracker. The evidence source records which inference was made, the panel
labels it, and the effect measures report the two sources separately so a deployment with
no effector reporting cannot mistake track deletions for confirmed effect.

**Re-engagement** is a new plan against the same track, not a mutation of the engagement.
The new plan cites the prior decision so the queue can show "second engagement" and the
operator sees the history.

## 6. Configuration and interface delta

`ConfigBaseline.assessment` gains `effect_window_s: f64` per effector layer, the period
after commitment inside which an effect is expected. Validated as finite and positive.

Interface:

- `CommandEvent::Decided` gains `decision: DecisionId`. Additive.
- `Event` gains `Engagement(EngagementEvent)` with `Opened`, `Executing`, `Closed { state }`.
- `SnapshotResponse` gains `engagements: Vec<Engagement>`.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-05 Recommendation panel | Open engagements against the same track, so a second recommendation is visibly a second engagement |
| PN-04 Track detail | Engagement history for the track with the evidence source per outcome |
| PN-08 Alerts | An engagement closing `Ineffective` or `Indeterminate` raises an alert; a successful one does not, because success is not an interruption |
| PN-17 Commander summary | Outcomes for the period, with effector-reported and track-inferred counted separately |
| PN-13 Reports | Effect figures with their evidence sources, feeding MOE-01 |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-4.6 Track engagements and effects | Replay with scripted outcomes, plus a property test on the state machine | Every accepted decision opens exactly one engagement; no engagement closes without an evidence record; an unobserved outcome closes `Indeterminate` and never as success or failure; track-inferred and effector-reported outcomes are counted separately in the report | TT-01 and TT-02 sample sets with scripted track deletions |

## Traceability

GAP-043; CAP-4.6; MOE-01; MT-01 step 8; depends on DN-04 for the layer window and DN-07
for effector reports; `../ux/wireframes/WF-04-track-detail-evidence.puml`,
`WF-17-commander-summary.puml`; principles AP-02, AP-03, AP-10.

## 9. Amendment 1 -- **signed by the owner 2026-09-06**

Raised 2026-09-06; signed the same day. The same sign-off covers the code that conforms to it.

Raised by GAP-043 on 2026-09-06, by implementing §5's transition table. One row of it
needed a reading the note does not give.

**"The plan is superseded" means `InterceptEvent::PlanSuperseded`.** The table sends any
open engagement to `Aborted` when "the plan is superseded, or a person aborts with a
reason". The planner re-solves every tick and publishes a new `PlanProposed` whenever the
plan changes; if every new proposal counted as superseding the accepted one, an engagement
would be aborted the frame after it opened and nothing would ever reach an outcome. The
implementation aborts an engagement only when its plan is the subject of a
`PlanSuperseded` event -- today, DN-08 §5's baseline-validity supersession -- and treats a
new proposal as what it is: a proposal, which a person may or may not act on. A person
aborting with a reason is the PN-05 control §7 lists, not yet built.

Also recorded under this amendment: **no engagement opens without a configured effect
window.** §6 adds `effect_window_s` per layer; a decision on a layer with none opens no
engagement and raises an alert, rather than opening one against a guessed window that
would close on the guess.

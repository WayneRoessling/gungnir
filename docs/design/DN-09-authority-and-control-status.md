# DN-09 Weapons control status and engagement authority

Closes GAP-033. Status: **signed off by the owner 2026-09-05.** Design only; no code exists yet.
**Human-owned and signed**: `gungnir-policy` is a low-trust crate and this note defines
who may engage what. The owner signed it on 2026-09-05; it may now be implemented, and a
change to it is a change request under phase H rather than an edit.

## 1. The gap and the thread step it blocks

`PolicyChain` checks geofences and resource readiness. It does not know weapons control
status, and it does not know that the authority matrix reserves an area-layer engagement
to a supervisor. So a recommendation can be presented as actionable to a role that lacks
the authority, or while the layer is at hold. MT-01, MT-02, and MT-07 all depend on the
distinction.

## 2. The owning component

`gungnir-policy`, which owns `PolicyEngine` and the chain. `gungnir-model` owns
`WeaponsControlStatus`, because DN-08's configuration types carry it and the interface
publishes it.

`gungnir-policy` depends on `gungnir-model` and `gungnir-geo`. **No new edge.**

## 3. Types

In `gungnir-model`:

```rust
/// Weapons control status, per effector layer. Ordered from most to least
/// permissive so that a comparison reads the way the doctrine does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default,
         serde::Serialize, serde::Deserialize)]
pub enum WeaponsControlStatus {
    /// Engage anything not positively identified as friendly.
    Free,
    /// Engage only what is identified hostile, or what meets the declared criteria.
    Tight,
    /// Engage nothing without an explicit order.
    #[default]
    Hold,
}
```

`Hold` is the default and the derive order puts it last, so a partially configured
deployment is at hold rather than free. That single choice carries more safety weight than
anything else in this note.

In `gungnir-policy`:

```rust
/// Denies a plan whose layer is at a status that forbids it.
pub struct ControlStatusPolicy<'a> {
    pub settings: &'a ControlStatusSettings,
    pub resources: &'a [ResourceView],
}

/// Denies a plan the asking role may not accept, given the layer and the
/// classification of the track.
pub struct AuthorityPolicy<'a> {
    pub settings: &'a AuthoritySettings,
    pub asking_role: Role,
}
```

`DenialReason` gains `ControlStatus { layer, status }` and `Authority { required_role }`.
Both carry what the panel needs to explain the denial, because "denied" without a reason
sends the operator to a radio to ask why.

## 4. Edges

**None.** `Role` comes from `gungnir-security`, and `gungnir-policy` does not depend on it
today. The design uses the role's canonical **name** as a string in `AuthorityRule`, which
DN-08 validates against the adopted set at configuration load. That keeps policy free of a
security dependency and puts the spelling check where the configuration is validated,
which is where a misspelling can be reported to a person.

## 5. Behaviour

**Control status.** For each solution in a plan, look up the resource's layer, look up that
layer's status, and:

| Status | Rule |
|---|---|
| `Free` | Permitted unless the track is classified `Friendly` |
| `Tight` | Permitted only if the track is classified `Hostile` |
| `Hold` | Denied |

A layer with no configured status is `Hold` (DN-08's defaults table). A plan whose
solutions span layers is judged per solution, and the plan is denied if any solution is.

**Authority.** The chain is evaluated for a specific asking role, so the same plan can be
actionable for a supervisor and not for an operator. That is the point: the queue shows an
operator what they may decide and marks what must go up.

The rule: find the `AuthorityRule` entries matching the action, the layer, and the class.
Most specific wins, in the order class-and-layer, layer, class, neither. **If no rule
matches, the answer is denied**, per DN-08's defaults table. Pre-delegated rules permit
without escalation, which is D-15's mechanism.

**Two properties that must hold and are tested rather than assumed:**

1. **No verdict is ever "permitted to act".** `PolicyVerdict`'s existing shape already
   requires a human decision on a clean plan, and this note does not add a path around it.
   Contract C-01.
2. **A denial explains itself.** Every new `DenialReason` variant carries the layer, the
   status, or the required role.

**The authority matrix is the test fixture.** `../mission/roles-and-stakeholders.md` §4 is
a table of role against decision. Every cell becomes a test case, which is what MOP-38
measures and what makes GAP-058 implementable rather than approximate.

## 6. Configuration and interface delta

Everything comes from DN-08's `ControlStatusSettings` and `AuthoritySettings`. This note
adds no configuration of its own, which is why DN-08 is designed first.

Interface:

- `SnapshotResponse` gains `control_status: BTreeMap<EffectorLayer, WeaponsControlStatus>`,
  because the status strip must show it and a peer needs it to interpret the picture.
- Setting the status is an authorized action, `weapons.control_status`, added to
  `gungnir_security::actions`, and is itself a recorded decision.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-01 Status strip | Status per layer, permanently visible. This is the single most important thing on the strip |
| PN-05 Recommendation panel | Denial reason in words when a plan is blocked by status or authority |
| PN-06 Approval queue | Items the asking role may not accept are marked as requiring escalation rather than hidden, so the operator knows the queue is longer than what they can act on |
| PN-17 Commander summary | Status changes in the period, with who made them |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-3.6 Rules of engagement | Table-driven test over every authority-matrix cell, plus scenario replay per status | Every cell of the authority matrix is exercised and matches (MOP-38); an unconfigured layer is at `Hold`; no role without a matching rule may accept; a plan denied by status or authority carries a reason naming the layer, the status, or the required role; no verdict permits action without a human decision | The authority matrix; TT-01 and TT-02 replays at each status |

## Traceability

GAP-033, and the specification GAP-058 implements against; CAP-3.6, CAP-6.2; D-05, D-15;
MOP-38; `../mission/roles-and-stakeholders.md` §4;
`../ux/wireframes/WF-01-status-strip.puml`, `WF-06-approval-queue.puml`; contracts C-01,
C-04; depends on DN-04 and DN-08.

# DN-09 Weapons control status and engagement authority

Closes GAP-033. Status: design only; no code exists yet.
**Human-owned**: `gungnir-policy` is a low-trust crate and this note defines
who may engage what. A change to it is a change request under phase H rather than an edit.
What the owner has signed of this note is in [`../signatures.md`](../signatures.md).

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

## 9. Amendment 1 -- a layer at hold refusing every plan is said (2026-09-26)

**Raised by GAP-183; decided by the owner as D-114.** §5 refuses a plan whole when any
of its solutions is refused, and the allocator tasks every adequate resource, so a
deployment with one layer at hold is offered nothing on any layer while every plan
tasks that layer: round 1 (area hold, point free) is such a deployment. **The rule
stands.** What changes is that it is said, so an operator never reads the empty queue as
a quiet sector.

1. **PN-06 says it, while it is true.** For each layer at hold that has refused a plan
   since the last plan offered for decision, and is still at hold under the policy in
   force: the layer; how many plans its hold refused, of how many evaluated since its
   first refusal; that none has been offered since; that the other layers' engagements
   in those plans were refused with them (§5's whole-plan rule); and that lifting the
   hold on that layer -- a supervisor's or commander's act -- is what would let them
   through. The line goes the moment a plan is offered, or the layer's hold is lifted.
2. **PN-05 says it beside the plan** that tasks a held layer: that plan will not reach
   the approval queue, in PN-06's own sentence (§7 already asks PN-05 for the denial in
   words).
3. **Whichever machine holds the queue counts, and says.** The approval desk keeps the
   window (`gungnir_approval::DenialHistory`), publishes `InterceptEvent::HeldLayers`
   with `gungnir_model::HeldLayerView`s whenever the list changes, and a node carries the
   last list in its snapshot's `held_layers`, so a linked desktop draws the node's count
   and not one of its own.

The details in rules 1 to 3 beyond the owner's words were taken under the owner's
delegation and are recorded with D-114. This note is human-owned; what the owner has
signed of this amendment is in [`../signatures.md`](../signatures.md). Reasoning:
`../record/2026-09-26/a-layer-at-hold-refusing-every-plan-is.md`.

## 10. Amendment 2 -- the status changes live, and the record says who (2026-10-26)

**Raised by GAP-203 (plan 12, tranche A); the owner's choices of 2026-10-26 and D-140.**
§6 said that setting the status is an authorized action and itself a recorded decision,
and §7 asked PN-01 to show the status and PN-17 to list the changes with who made them;
until this amendment the only write path was a baseline apply that took effect at the
next start (`gungnir-app/src/sustainment.rs`), so a raid that began under `Hold` stayed
under `Hold` until somebody edited a file and restarted the console. **The rule of §5
stands.** What changes is that the status is a live, recorded act:

1. **The act.** A signed-in supervisor or commander puts one layer at one status, with a
   reason, from the status dialog that PN-01's chip opens (FL-08). The change is
   `gungnir_model::ControlStatusChange`: the layer, the new and the previous status, the
   reason, the person and the role their authenticated session carried, the baseline
   revision it was made under, the machine it was asked from, and the second person
   where rule 4 applies. It is journaled as `PolicyEvent::ControlStatusSet`, audited
   under `weapons.control_status`, and applied to the settings in force in the same act,
   through `gungnir_approval::ApprovalDesk::set_control_status`, which both binaries run.
2. **Where it is applied.** On the machine that holds the queue (D-55): a node for the
   desktops linked to it, through `POST /v3/policy/control-status`, which answers `201`
   with the change and the items it withdrew; a desktop for itself while it runs its own
   queue. A linked desktop with the link up sends the request to the node and records
   nothing, and the node's change reaches every linked console on the stream and in the
   snapshot's `control_status`. **Cut off, a desktop changes its own status** and journals
   it, because it runs its own queue; on reconnect the node's status is in force again,
   and the offline change is listed on PN-18, neither forwarded nor arbitrated.
3. **The queue's answer.** A tightening re-judges every queued item by §5's control-status
   engine alone, and an item the new status refuses leaves the queue as
   `CommandEvent::Withdrawn`, with a record that is neither a decision nor an expiry:
   nobody chose, the policy changed under it. A loosening resurrects nothing; the next plan
   is judged under the new status. PN-06's ended rows and a `409` on a decision taken too
   late say the withdrawal by name.
4. **Two people where the baseline says so (D-140).** `policy.two_person.control_status`,
   off by default, requires a second signed-in person who holds `weapons.control_status`
   and is somebody else to confirm with their own credential, verified against the same
   store and under the same back-off as a sign-in; the record and the audit entry name
   both. Where the setting is off, single approval is the rule, as a recorded decision
   rather than an omission. The override half of D-140 is its own gap.
5. **Across a restart.** The status in force at start is the baseline's value overridden by
   the latest journaled change made under that baseline's revision; a later revision
   supersedes every live change made before it, because a baseline that rewrites the
   status is a change request under phase H (§1). The baseline file is never rewritten.
6. **On the strip and on PN-17.** PN-01's chip says who set a layer and when and opens the
   dialog for a role that may change it; PN-17 lists the session's changes with who made
   them, which §7 asked for and nothing supplied until now.

The four choices in rules 2, 3 and 5 were the owner's on 2026-10-26, each taken to the
recommended default; rule 4 is D-140. This note is human-owned; what the owner has signed
of this amendment is in [`../signatures.md`](../signatures.md). Verification:
`../verification-capability-table.md` §2, the GAP-203 row. Reasoning:
`../record/2026-10-26/gap-203-the-status-changes-live.md`.
## Traceability

GAP-033, GAP-203, and the specification GAP-058 implements against; CAP-3.6, CAP-6.2; D-05, D-15, D-140;
MOP-38; `../mission/roles-and-stakeholders.md` §4;
`../ux/wireframes/WF-01-status-strip.puml`, `WF-06-approval-queue.puml`; contracts C-01,
C-04; depends on DN-04 and DN-08.

# A layer at hold refusing every plan is said

GAP-183 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-114 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
[`../../design/DN-09-authority-and-control-status.md`](../../design/DN-09-authority-and-control-status.md)
§9. DN-09 is a human-owned note; what the owner has signed of it is in
[`../../signatures.md`](../../signatures.md). Built on 2026-09-26.

## What was decided, and by whom

GAP-183 was filed while building GAP-182
([`a-rehearsal-decides-and-tracks-under-the-deployment.md`](a-rehearsal-decides-and-tracks-under-the-deployment.md)).
DN-09 refuses a plan whole if any of its solutions is refused, and the allocator tasks
every adequate resource. So one layer at hold refuses every plan that tasks it, and the
other layers' engagements in those plans go with it. Round 1 (area hold, point free) is
offered nothing.

**The owner decided it on 2026-09-26 (D-114): keep the rule, and say so.** The planner
and DN-09's whole-plan refusal are unchanged. PN-06 says plainly, while it is true, that
a layer at hold is refusing every plan. It names the layer and the count, and says that
lifting that layer's hold, a supervisor's or commander's act, is what would let the other
layers' engagements through. That way an operator never reads the empty queue as a quiet
sector.

## What was taken under the delegation, and why

The owner left three details open.

- **The window.** The counts run from a layer's first refusal after the last plan
  offered for decision, and the line goes the moment a plan is offered. "Refusing every
  plan" is then exact: every plan in the window was refused, and `refused` of the
  `evaluated` were refused by that layer's hold.
  - The view is also filtered, at read time, to layers still at hold under the policy in
    force, so a hold that has been lifted is never drawn as refusing.
  - *Since the layer went to hold* was rejected. A hold can refuse nothing for an hour
    while no plan tasks the layer, and a count from then says nothing about now.
- **PN-05 says it too.** Beside a plan that tasks a held layer, PN-05 says the plan will
  not reach the approval queue, and why, in PN-06's own sentence.
  - DN-09 §7 already asked PN-05 for a denial reason in words. This is the one denial
    that makes the whole panel's recommendation moot, so it is said on the plan it is
    about.
- **A linked desktop is told, not left to derive.** The count is kept by whichever
  machine holds the queue: its approval desk counts. It is published as a structured
  `HeldLayerView` in `InterceptEvent::HeldLayers` whenever the list changes, and carried
  in the snapshot's `held_layers`, as D-94 carries a plan's standing.
  - A linked desktop draws the node's list, count for count.
  - Rejected: deriving it on the desktop from `PlanEvaluated`'s reason string. That
    would parse a debug spelling, and a desktop that linked mid-window would count from
    its own arrival and understate what the node has refused.

Both additions follow the compatibility rules `docs/gungnir-api-v1.md` states: a new
enum variant, and a defaulted field. So `SCHEMA_VERSION` stands.

## How a hold is lifted today

A supervisor's or commander's act (`weapons.control_status`, §4). Today that act is
applying a baseline whose policy changes `control_status`, which takes effect on restart.
The tests lift a hold that way.

## Tests

- `gungnir-approval` `held_layer_tests`: the window's arithmetic, and a lifted hold.
- `gungnir-app/tests/held_layer_said.rs`:
  - an area-hold, point-free desktop names the area layer with its count on PN-06 and on
    PN-05, and with the hold lifted its plan is queued and neither line is drawn;
  - the line goes the moment a plan is offered;
  - round 1 as committed says its area hold is refusing every plan.
- `gungnir-app/tests/linked_plan_standing.rs`
  `a_linked_desktop_is_told_a_held_layer_refuses_every_plan`: a node over mutual TLS
  counts two refusals, and the linked desktop draws the same count and sentence.
- The model round trip and the full snapshot round trip carry the new type and field.

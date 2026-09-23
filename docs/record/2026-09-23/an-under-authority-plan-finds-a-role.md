# An under-authority plan finds a role

GAP-113 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-09 §7, DN-31 §6.1.

## What was wrong

DN-09 §7 has a plan the asking role may not accept marked for escalation to a role that
may. The desktop ran the policy chain for the role at the console, took the authority
denial, counted it and queued nothing -- so an area-layer plan asked by an Operator
reached nobody with the authority to take it. The queue stayed empty and a counter went
up, which is the least reviewable place a refusal can land.

The node has done the right thing since GAP-132: `submit_to_ladder` runs the chain for
every role on the escalation ladder and offers the item to the lowest one that may take
it (DN-31 §6.1). GAP-113's own history says so, and says the desktop half stayed open --
because a cut-off desktop decides from its own queue (DN-31 §6.7), so the rule has to hold
in both places.

## What was built

`ApprovalDesk::submit` still runs the chain for the role at the console, and still returns
what became of the plan. What changed is what happens to **one** of the four verdicts: an
authority denial. The authority engine is the only engine that reads the asking role at
all, so its refusal is the only one another role might not get; the other three deny for
everybody, and walking the ladder for them would ask the same question four more times
for the same answer.

So on an authority denial the desk walks the ladder, and if a role on it may accept, the
plan is queued offered to that role. `offered_to` on the item says who, which is what
PN-06 draws and what `may_be_decided_by` reads. A plan no role may accept is denied and
never queued, exactly as before: that is what makes the denial reviewable rather than
counted.

**The offer is the ladder's answer, whatever it is.** Authority rules name a role with a
layer and a class and need not climb, so the role that may accept is whoever the walk
finds -- the same rule the node applies, which is the point: the desk is one piece of
code and a desktop's queue and a node's must not come to disagree about who an item
belongs to.

## What the tests hold

`a_plan_this_console_may_not_accept_is_offered_to_a_role_that_may`
(`gungnir-app/tests/approval_gate.rs`) gives a desktop the matrix
`gungnir-app/tests/desktop_projection.rs` gives its node -- an Operator holds the point
layer, a Supervisor holds both -- asks an area-layer plan as the Operator at the console,
and finds it queued for the Supervisor with the denial counter still at zero. Its sibling
holds the case that did not change: a point plan stays offered to the console's own role
and is not handed past it.

## What it did not change

The verdict a person sees for their own role, the record the chain publishes, and the rule
that a denied plan is never queued. Nothing here decides anything or widens who may
decide: `decide` still asks both of its questions, and an item offered to a Supervisor is
refused to an Operator exactly as it was.

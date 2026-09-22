# Desktop decide and task authorize the call

GAP-127 closes. `gungnir-app`'s `decisions::decide` and `requirements::task` named
`plan.decide` and `sensor.task` on their audit entries and checked neither. PN-06 and the
requirements panel hid the controls a role may not use, so the rule held for a person at
the screen and for nothing else that calls the functions, while the node authorizes every
call on the caller's role. Both functions now ask the question themselves, against
`AppState::role`, which is the signed-in account's role whenever there is one (D-53).

**`decide` asks what the node's route asks, not only what the gap named.** The gap said
"check `plan.decide`". The node's `vet_decision` checks two things before a decision
reaches the queue: the permission the decision needs, which is `plan.override` for an
override and `plan.decide` otherwise; and whether the item was offered to that role, or
escalated to it (DN-10 §5). A desktop that checked only `plan.decide` would have stayed
weaker than the node in a way that matters. **Operator holds `plan.decide` and not
`plan.override`**, so until this change a desktop Operator could record an override the
node refuses with a 403. The desktop now refuses it too.

**A refusal records nothing.** Both checks run before the desk is touched: no decision in
the history, nothing published, no audit row, and the item still waiting. `task` checks
first of all, before it looks the requirement up and long before a command could be
issued, since a task cannot be withdrawn once sent.

**Two named refusals.** `gungnir-command`'s `CommandError` gains `NotPermitted { role,
action }` and `NotOffered { item, role, offered_to }`, so PN-07's alert says which rule
refused and names the roles the item is offered to. `RequirementError` gains its own
`NotPermitted`. The node's match on `CommandError` has a catch-all arm and is unaffected;
the desktop's is exhaustive and gains the two arms.

**One existing test changed, and why.** `requirements_replay.rs` signs out and expects the
"sign in to task it" refusal. After a sign-out the desktop acts in whatever role was
selected, and the previous step had left one that does not hold `sensor.task`, so the
permission refusal now comes first. The test selects `SensorManager` before the attempt,
which is the scenario it describes: a sensor manager's console with nobody signed in.

**Tests.** `approval_gate.rs` gains four: a role without `plan.decide` refused; an
Operator's override refused for want of `plan.override`; a Supervisor refused on an item
offered only to the Operator; and the Operator accepting it as the control. Each refusal
asserts nothing reached the history, the queue or the audit trail. `requirements.rs` gains
an Operator refused `sensor.task` with no command issued and the requirement still stated.

**Human-owned.** The decision path is human-owned wherever it runs (D-65), and
`gungnir-command` is human-owned by name, so this came to the owner as a review.

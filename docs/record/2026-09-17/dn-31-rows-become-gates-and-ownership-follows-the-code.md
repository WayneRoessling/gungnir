# Three DN-31 rows become gates, and the decision path's ownership follows it

Two answers from the owner on 2026-09-17, taken together because they are the two things
DN-31's build series had left with him. What he signed is in
[`../../signatures.md`](../../signatures.md).

## The three rows

DN-31 §9's ten rows were agreed when the note was written, before any of their code existed
(AP-17), and each was to become a gate only when the owner confirmed that its test checks
its criterion (D-16). Three now have tests that pass on `main`, and he confirmed all three
against the tests named in their own cells:

- **Row 1, identifiers (CAP-7.2), built in GAP-130.** Four clauses, four tests:
  `gungnir-command/tests/identifiers.rs` and `gungnir-intercept-service/tests/identifiers.rs`
  stand separate workflows and planners in for a node, two desktops and a restart;
  `gungnir-app/tests/report_reaches_one_handoff.rs` sends an effector's report through the
  real route and both desktops' links; `measures.rs` holds MOE-05's cross-machine join; and
  `pre_uuid_v7_journal.rs` replays a journal written before the change and compares the
  regenerated report byte for byte.
- **Row 2, the decision path in one place (CAP-4.2), built in GAP-131.** The workspace suite
  passes with the only edited assertions being the two path pins DN-31 §3 sanctions;
  `no_execution_without_decision.rs` scans every `gungnir-*/src` for a second handoff
  builder; and DN-10's exhaustive `no_settings_can_make_an_expiry_accept` still passes,
  untouched by the move.
- **Row 10, the desktop alone (CAP-5.4), built in GAP-131.** The existing no-node fixtures --
  `approval_gate.rs`, `engagements.rs`, `endpoint_delivery.rs` -- pass with their assertions
  untouched, which is what "unchanged" has to mean for a row whose whole content is that
  nothing changed.

Rows 3 to 6 have tests since GAP-132 and wait on the same confirmation; rows 7 to 9 wait on
the gaps that build them. The table says which a row is, in the row.

## The ownership question

`docs/agentic-workflow.md`'s recommend-versus-act boundary named `gungnir-policy`'s verdict
logic and `gungnir-command`'s approval workflow. DN-31 moved the rest of that path out of
`gungnir-app` -- which the list never named, because the path had always been in a binary --
into `gungnir-approval` (GAP-131), and then on to a node (GAP-132), which the list never
contemplated at all. So the code that opens an engagement behind an actionable decision, and
the one place a handoff is built, were outside the list for two increments.

**The owner's answer (D-65): the list follows the code.** `gungnir-approval` and the node
loop that runs it are human-owned. The reasoning he gave is the one the rule already carries:
it is about a boundary -- nothing executes an intercept without a recorded human decision --
rather than about the three crates that happened to hold it, so code that moves the boundary
moves the ownership with it.

This was raised rather than folded into the change that moved it, on the precedent the same
list records for `gungnir-remote`'s identity path: GAP-060 moved the certificate and key
custody out of `gungnir-node` into a crate the list did not name, and it was brought to the
owner rather than assumed. That entry says to bring the borderline ones and not to decide the
edge unilaterally. This was not even borderline -- the path named in the rule had moved --
which is why it was worth asking rather than worth editing.

## The code those rows run against

A confirmed criterion is a statement about a test, not about the code the test runs against,
so the human-owned code of the three increments was taken up separately in the same session:
GAP-130's identifiers and the `/v3` move, GAP-131's move of the decision path into
`gungnir-approval`, and GAP-132's node loop and queue routes. Each covers named paths at the
commit on `main` that carries them, which is what makes a row of the ledger checkable.
[`../../signatures.md`](../../signatures.md) is the only place that says what the owner has
put his name to, and this item does not restate it.

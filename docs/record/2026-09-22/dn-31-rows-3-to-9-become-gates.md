# DN-31 rows 3 to 9 become gates

The owner's confirmations of 2026-09-22, and the change they required in row 7's test.
What he signed is in [`../../signatures.md`](../../signatures.md).

## All ten rows now gate

DN-31 §9's ten rows were agreed on 2026-09-17, when the note was written and none of their
code existed (AP-17). Under D-16 each becomes a gate only when the owner confirms that its
test checks its criterion. Rows 1, 2 and 10 were confirmed that day; rows 3 to 9 are
confirmed now, each against the tests its own cell in
[`../../verification-capability-table.md`](../../verification-capability-table.md) names:

| Row | Confirmed against |
|---|---|
| 3, one decision per item | `approval_queue.rs`'s hundred-round race over the real transport |
| 4, authorization on the node | the refusal matrix, with one audit entry per decision *and* per refusal, and the read-side test that a role not offered an item may still see the queue |
| 5, authority and offering | the offering rule and the denial that names Authority |
| 6, expiry and escalation | escalation keeping the first role, an expired item refused, a pre-delegated item still expiring (D-59) |
| 7, two desktops, one queue | the projection test, including MOP-07 (below) |
| 8, cut off and reconnected | the outage test: exactly once, forwarding twice recording nothing, the lapse, rule-or-person, and `BothActed` whatever the verdict |
| 9, MT-01 with a watch floor | the saturated-queue test across three consoles |

## Row 7's timing clause, and where it is now enforced

Row 7's fourth clause is MOP-07: a plan proposed on the node available to decide on a
desktop in under 500 ms. That clause was asserted from GAP-133 until 2026-09-17, when a run
measured 647.1 ms on a change that touched nothing on the path and the assertion was
reduced to a printed measurement — a debug-profile timing bound on a runner shared with
every other test binary measures the runner, not the path
(`../2026-09-17/row-7-mop-07-is-measured-until-the-row-is-gated.md`).

Gating it puts the assertion back, in the release profile alone, which is the convention
`gungnir-app/tests/frame_budgets.rs` states for a budget and the form its own gates take:
`cfg!(debug_assertions)` prints in debug and asserts in release.

**A release-only gate is only a gate if something runs it in release.** `ci.yml` already
had a step for exactly that reason — "a release-only budget that no job runs in release is
a gate nothing enforces. It must stay" — running `frame_budgets` with `--release`. That
step now runs `desktop_projection` beside it, on the release-profile dependencies it
already builds.

## What did not change

The criteria themselves. Row 7's text still reads "under 500 ms (MOP-07)", and no row's
wording moved: a confirmation says a test checks a criterion, not that the criterion has
been rephrased.

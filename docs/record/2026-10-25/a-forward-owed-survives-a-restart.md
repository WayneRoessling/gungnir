# A forward owed survives a restart

GAP-202, found closing GAP-194. A desktop cut off from its node records a commander's gap
acceptance and forwards it when the node answers, as D-131 requires. Before this change
the forward was queued only at the moment the acceptance was made, and the queue was held
in memory. So a desktop restarted before its node answered still had the acceptance on its
own journal, but the node never heard of it: every other console showed the gap as
unexamined while this one showed it accepted. An outage's decisions never had this
problem, because GAP-142 recovers them from the journal.

## What was built

- **Owed forwards.** `GapAcceptances` keeps the acceptances it owes its node a forward,
  until the node answers. An acceptance made while cut off is owed at once.
- **At start.** On the connected profile, every standing acceptance recovered from the
  journal is owed again (`GapAcceptances::owe_all`), because none of the forwards that
  were waiting survived the restart.
  - This re-sends any the node already held, and that is deliberate. Telling the two
    apart would need a record of which forwards were answered, and the node answers a
    repeat `already-accepted` at no cost. The recovered ledger already holds every
    acceptance this desktop made.
  - A desktop on the standalone profile owes nothing, because it has no node.
- **`forward_owed`, every frame.**
  - **Queuing.** It keeps each owed acceptance in the current link's outbox, so a link
    rebuilt by signing in again picks them up too.
  - **Pruning.** An owed acceptance that no longer stands here, because it re-opened for a
    reason of its own, is owed nothing and is dropped.
  - **Who may forward.** It queues only while a commander is signed in. The route takes a
    forward under the signed-in operator's token and refuses any role without
    `coverage.accept_gap`. That refusal is about who is at the console, not about the
    acceptance.
- **Answers, corrected.** Two answers were mishandled by GAP-194's first build:
  - **`already-accepted` now settles a forward.** The node holding the gap is exactly what
    was wanted. Before, this answer was treated as a refusal and re-opened the desktop's
    acceptance, and a repeat after a restart would have hit that every time.
  - **A `401` or `403` leaves the acceptance owed.** Before, it re-opened the acceptance,
    though it says only that the signed-in role may not forward. Every other refusal still
    re-opens it with the node's reason (D-134).

## Checks

`gungnir-app/tests/linked_gap_acceptance.rs` covers both restart cases, against the real
transport and `gungnir_node`'s own ledger:

- **A restart before the node answers.** The desktop owes the forward again after the
  restart and queues it. When the node comes up, it holds the acceptance as the commander's,
  and the forward settles.
- **A restart after the node already holds it.** The repeat is answered
  `already-accepted`. The acceptance stands, nothing re-opens, and the answer is said.

GAP-194's three end-to-end tests run unchanged.

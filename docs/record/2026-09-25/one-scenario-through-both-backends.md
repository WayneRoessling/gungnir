# One scenario through both backends

GAP-120, GAP-160 and GAP-161
([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-85 and D-86 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
taken under the owner's delegation of 2026-09-25. GAP-120 was filed by the GAP-067 walk
([`../2026-09-16/gap-067-walk.md`](../2026-09-16/gap-067-walk.md)), which held the
`gungnir-app` backend-switching row of `docs/verification-capability-table.md` §2 because
nothing compared the embedded and the remote backends' projections.

## What was built

`gungnir-app/tests/backend_parity.rs` gives one generated Scenario 1 timeline
(`gungnir-scenario`, seed 1, written as a recorded feed) to two desktops on one baseline.
Desktop A runs its own services. Desktop B signs in to a node and runs remote: its gateway
forwards every detection through the link, the node tracks and plans, and B draws what the
node's stream and snapshot say. The link is the product's mutual TLS: B presents the
identity it issued itself at start (GAP-141), the node serves under an identity issued from
a key provider as the binary issues one (D-29) and pins B's certificate as its client
authority, and B's baseline pins the node's. Nothing is signed by a test authority.

The node side is `gungnir-node`'s own code. Its tracker, planner, gateway, submission
adapter, published picture and -- new here -- what it announces about the picture are all
built by `gungnir_node::picture`, a library module the binary now calls, as
`gungnir_node::approval` is. The test writes only the loop's order, the one `main.rs` runs.

Both sides drain on what the system reports, never on a pause (GAP-136): every detection has
reached both trackers (both gateways took the whole feed, B's outbox is empty, the node's
gateway took exactly what B's accepted); both pipelines are finished and each host is ticked
until its tracker reports unhealthy, which `LiveTrackingService::poll` does only after
applying every snapshot the task sent, the flush included; the node is stepped until a step
publishes nothing; B is ticked until its link has applied the node's last envelope.

## What the comparison found

**A node published no track at all (GAP-160).** `TrackingEvent` has five readers -- a
linked desktop's projection, a partner's stream, the reports, the replay and the node's
entity fold -- and no producer anywhere in the workspace. A desktop linked to a node kept
the picture its sign-in snapshot held for as long as the link stayed up: without the fix
the test draws no track on B while A draws three. The node now diffs its picture each tick
against what it last announced and publishes the difference (D-86):
`gungnir_tracking_service::TrackLifecycle`, carried by `picture::Announcer`.

**A linked desktop's health was the link's (GAP-161).** Both remote services answered
`is_healthy` with whether the link was up. The node reports its services twice over -- the
snapshot's `health` and `HealthEvent::Changed` on every transition -- and nothing read
either, so a node whose tracker had stopped was drawn tracking on every linked status strip.
The link now keeps the node's word and each remote service is healthy when the link is up
and the node says its service is. `gungnir-remote/tests/node_health.rs` pins it; three
existing transport fixtures served `SystemHealth::default()`, every service down, and now
serve a healthy node, because what they test is the link.

**The node planned without the deployment's frame.** The binary built its planner with
`DpInterceptService::new` alone, while the desktop's embedded backend added the local frame
(GAP-031), so every plan a node proposed carried no intercept point and no time to
intercept where an embedded desktop's carried both. `picture::intercept_service` builds it
with the frame. Not given a gap of its own: it was found and fixed in the change that found
it.

Each of the three was checked by undoing its fix and running the test: each undo fails the
test at the comparison it concerns, and nothing else.

**A node test waited on a pause.** Running `gungnir-node/tests/approval_queue.rs` beside
this work, `a_pre_delegated_item_still_expires_and_escalates` failed twice by indexing an
empty queue: the harness read the node's published queue after a fixed 20 ms `settle()`,
a guess at when the loop thread would next run, and under load it had not. `settle` now
waits for the loop to finish a whole tick after the call, on a counter the loop keeps --
the rule GAP-136 set for the rehearsal, applied to the harness.

## How the comparison is judged (D-85)

Tracks, retained bearings, PN-09's bearing counters, the health strip, the withheld
resources, the alerts the scenario raised and the what-if PN-05 shows for a selected track
are compared exactly. B's plan is compared exactly with the plan the node holds.

A's plan and B's are compared by their pairing -- which resource on which track -- and the
two backends' planners by a common solve. A plan keeps the geometry, value and time of the
solve that first made its pairing (GAP-097), and which solve that was depends on which frame
first saw the asynchronous pipeline's report. Two runs of the same embedded desktop can
therefore hold one pairing under different geometry; the difference belongs to the frame
schedule, not to the backend. So the question those fields answer -- does the node plan as
the desktop does -- is asked where it has an exact answer: each backend's planner, built by
its own construction path, solves the one final picture at one time, and the two plans must
be equal in everything but the identifier each minted. That is the comparison that caught
the missing frame; comparing plan vintages would have hidden it behind the scheduler.

## What the walk should know

The row is left for the owner's walk; this change does not gate it or change its criterion.
Four things the walk will want in front of it:

- **The alert comparison is empty on both sides.** The default baseline enables no anomaly
  detector and Scenario 1 raises no other alert on either desktop, so identical is "none and
  none". An alert raised from an intermediate picture -- a kinematic anomaly -- would depend
  on the frame schedule the way plan vintage does.
- **Retained bearings and PN-09's counters reach a linked desktop at connection time only**
  (`link::Projection::bearing_rays`, a documented choice). Scenario 1 carries no bearing, so
  both sides read empty and zero; a scenario with bearings would show a linked desktop's
  counters frozen at sign-in.
- **A linked desktop reports no withheld resource by design** (`InterceptService::withheld`:
  a remote planner does not know what the node held back). The scenario's one resource is
  adequate on both sides, so both read none.
- **Scenario 1 ends with three tentative tracks** after 131 initiations over 292 detections
  on both backends alike: parity holds, but the default filter does not hold this target as
  one confirmed track. That is the tracking rows' question, not this one's.

A further finding outside the test's path, not filed because this change's two reserved
gap identifiers were used: when a baseline names no session lifetime, a desktop's own
session never expires (DN-23 §5) while the node's token lasts 900 s
(`gungnir-node/src/auth.rs`), and nothing renews a token while the link's stream stays up.
From then on the node refuses the link's writes `401`: detections and exchange publishes
wait in their outboxes until the stream happens to reconnect, and every decision is answered
with the refusal.

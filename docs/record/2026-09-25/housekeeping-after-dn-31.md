# Housekeeping after DN-31

GAP-129, GAP-138 and GAP-139
([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml));
D-79 and D-80; GAP-154 filed. Three gaps the DN-31 build left behind, taken under the
owner's delegation of 2026-09-25.

## GAP-129: the parent gap of a built note

GAP-129 was still "In progress" though the five gaps that built DN-31 (GAP-130 to GAP-134)
and the gaps that followed them (GAP-135, GAP-137, GAP-140 to GAP-143, GAP-145) were
closed. It was checked against the code rather than against the register:

- **Every one of DN-31 §9's ten rows has its test on `main`.** Each file and each named
  test function in the verification table's "Rows added by DN-31" cells was found at
  12a14751:

  | Row | Test |
  |---|---|
  | 1 Identifiers | `gungnir-command/tests/identifiers.rs`, `gungnir-intercept-service/tests/identifiers.rs`, `gungnir-app/tests/report_reaches_one_handoff.rs`, `gungnir-app/tests/measures.rs` (MOE-05), `gungnir-app/tests/pre_uuid_v7_journal.rs` |
  | 2 The decision path in one place | `gungnir-app/tests/no_execution_without_decision.rs`; `gungnir-command/src/queue.rs`, `no_settings_can_make_an_expiry_accept` |
  | 3 One decision per item | `gungnir-node/tests/approval_queue.rs`, `two_clients_racing_one_item_produce_exactly_one_decision` |
  | 4 Authorization on the node | the same file, `every_refusal_records_nothing_and_writes_one_audit_entry` and `a_role_the_item_is_not_offered_to_may_still_read_the_queue` |
  | 5 Authority and offering | the same file, `each_item_is_offered_to_the_lowest_role_that_may_take_it` and `a_plan_no_role_may_accept_is_denied_by_authority_on_the_record` |
  | 6 Expiry and escalation | the same file, `an_item_escalates_without_losing_its_first_role_and_the_higher_role_decides_it`, `an_expired_item_is_refused_and_recorded_as_expired_never_accepted` and `a_pre_delegated_item_still_expires_and_escalates` |
  | 7 Two desktops, one queue | `gungnir-app/tests/desktop_projection.rs`, `two_desktops_show_one_node_queue_and_neither_decides_it_itself` |
  | 8 Cut off and reconnected | `gungnir-app/tests/cut_off_and_reconnected.rs`, `an_outage_decided_offline_reaches_the_node_once_and_what_both_sides_did_is_faced` |
  | 9 MT-01 with a watch floor | `gungnir-app/tests/mt01_watch_floor.rs`, `a_saturated_node_queue_worked_by_three_consoles_ends_every_item_once` |
  | 10 The desktop alone | `gungnir-app/tests/approval_gate.rs`, `gungnir-app/tests/engagements.rs`, `gungnir-app/tests/endpoint_delivery.rs` |

- **The plan is built.** §7's routes are served (`GET /v3/queue`,
  `POST /v3/queue/{item}/decision`, `POST /v3/decisions/forwarded`, `/v2` answering
  `410 Gone`); `SnapshotResponse` carries the queue; `policy.delegation.disconnected_lapse_s`
  is read by the fallback; §8's panels carry the node's queue, including PN-17's
  escalations and decisions by role. `docs/unbuilt.md`, generated from every
  `NotImplemented` the code returns, names nothing in DN-31. §11's three open items --
  substituted assignments, GAP-127 on a cut-off desktop (itself closed), several nodes --
  are outside the note's scope rather than unbuilt parts of it.

What the gap's name said -- "reconciliation never meets a conflict" -- is answered by
DN-31 amendment 2's finding rather than contradicted by it: unique identifiers, and a
linked desktop queueing no node plan, mean an outage no longer produces a plan conflict
at all, and two actions on one track are caught by the comparison by track (D-58), which
row 8 exercises end to end. The rows' gating is the owner's (D-16) and was not touched.

**Found on the way: GAP-154.** The cross-layer Disconnected reconciliation row still ends
by saying no build puts a decision on a node's record, citing GAP-129. The sentence sits
in the row's pass-criterion cell, and a criterion cell changes only on the owner's walk,
so it is filed rather than edited.

## GAP-138: one description of a decision (D-80)

`gungnir_api::v3::ApprovalRequest` -- a decision keyed on a plan, naming its own operator
in the body -- had been reachable from no route since GAP-132, and the one thing naming
it, `ApiHandler::decide`, had no implementor. Both are removed. The alternative, saying in
the trait what implements it and when, was rejected because nothing will: DN-31 §7 keys a
decision on the queue item, which carries the deadline and the roles, and a plan-keyed
description of the same act is a second contract every reader has to be told not to
believe. `cargo check --workspace --all-targets` confirmed that nothing reached either.

`ApiHandler` keeps `snapshot` and `submit_detection`, both of which describe routes the
node serves, and its documentation now says what is true of it: nothing implements it,
because the v3 transport serves every route through `transport::NodeApi`. The module
documentation of `transport.rs`, the comment on the unauthenticated-route test in
`gungnir-remote`, and `docs/gungnir-api-v1.md` stop describing the type as live.

**The UAF.** IE-23 is the operator's decision carried over the API, and five hand-drawn
views name it (If-Cn, If-Tx, Op-Cn, Sv-Cn, Sv-Pr). The element persists; the type that carries it
changed, so IE-23 now names `gungnir_api::v3::DecisionRequest` and says what it replaced.
IE-22, IE-24 and IE-25 named a `v1` module that has not existed since 2026-09-05 and now
name `v3`. `build_uaf.py`, `export_xmi.py` and `export_ea_script.py` were re-run.

Human-owned: `ApiHandler::decide` was on the `gungnir-api` write path. What the owner has
reviewed is in [`../../signatures.md`](../../signatures.md).

## GAP-139: one engine for the If-Sr family (D-79)

`build_uaf.py` wrote `!pragma layout smetana` on every If-Sr diagram, and smetana, the Java
port of Graphviz dot inside PlantUML, throws `ArrayIndexOutOfBoundsException` in
`mincross__c.left2right` on the plans-effectors-handoff diagram since it grew by one class.
That SVG was committed as a Graphviz render; the other eight were smetana's.

**Measured before deciding**, with `plantuml/plantuml:1.2026.8` on every If-Sr source under
both engines:

| Diagram | smetana | Graphviz dot | Area ratio |
|---|---|---|---|
| assets-exchange-releasability | 1255 x 1053 | 1298 x 1161 | 1.14 |
| battle-rhythm-mission-records | 1544 x 1243 | 1610 x 1444 | 1.21 |
| core-identifiers-frames-quality | 801 x 622 | 823 x 664 | 1.10 |
| events-the-record | 4861 x 2959 | 4904 x 3462 | 1.18 |
| overview | 1854 x 502 | 1997 x 773 | 1.66 |
| picture-tracking-vocabulary | 1494 x 1678 | 1516 x 2020 | 1.22 |
| plans-effectors-handoff | crashes | 2946 x 1151 | -- |
| policy-authority-settings | 2145 x 1776 | 2212 x 2071 | 1.20 |
| uas-identification-platform-reports | 1351 x 701 | 1394 x 712 | 1.05 |

The generator's comment justified smetana by a 30-40x width-to-height ratio. That figure
was the top-to-bottom default, and `left to right direction` cures it under either engine:
dot's layouts are as square as smetana's or squarer, at 5 to 66 per cent more area.

**The three options the gap named:**

- *Pin the engine per diagram, Graphviz for the one that crashes.* Rejected: it keeps two
  engines in one family, which is the defect.
- *Pin a PlantUML version with the defect fixed.* Rejected: none exists. `latest` is
  1.2026.8, the version that crashes, and smetana would stay one graph change away from
  the next crash while the registry check needs every element positioned.
- *Drop the pragma for If-Sr.* Taken, for the whole family, the overview included.

The render scripts now pin the image, `plantuml/plantuml:1.2026.8`, which had been
unpinned: the engine and its version are part of what a render looks like, and each SVG
records the version that drew it. All nine If-Sr sources were re-rendered from that image
and each exits 0; the plans-effectors-handoff SVG came out byte-identical to the one
GAP-132 committed. The Rs-Cn family keeps smetana: all twelve of its diagrams render
cleanly under the same image, and its comment's comparison was of hub-and-stub layouts,
which this change did not re-measure.

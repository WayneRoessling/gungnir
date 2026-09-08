# DN-03 Warning function

Closes GAP-042. Status: first draft 2026-09-05; **implemented the same day** (`gungnir-workflow/src/warning.rs`) and **wired on 2026-09-06** through a ledger the desktop tick evaluates. Delivery to an `http` endpoint is real (GAP-040's transport); a refusal or silence fails loudly (§5). **Amendment 1 (§9) gives the pass-close trigger its distance, and amendment 2 (§10) its due time, both signed by the owner 2026-09-06** (corrected 2026-09-07: this line had called amendment 1 unsigned a full day after §9's own header recorded the signature). **Amendment 3 (§11, how an acknowledgement arrives) is signed by the owner 2026-09-07.**

## 1. The gap and the thread step it blocks

For a threat with no engagement option, warning is the only response. MT-02 warns an asset
and its units; MT-04 warns a port authority. No component turns a predicted impact into a
warning with a lead time, a channel, and a record, so today it is a radio call an operator
remembers to make.

## 2. The owning component

`gungnir-workflow`, which already owns `AlertLifecycle` and its state machine. A warning is
an alert with an obligation attached and a delivery record, not a new mechanism.

## 3. Types

In `gungnir-workflow`:

```rust
/// A warning owed to an asset, raised when a prediction crosses the obligation's
/// lead time. It is an alert with three extra facts: who is owed it, by when, and
/// whether it went out.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Warning {
    pub asset: AssetId,
    pub track: TrackId,
    /// Mission time by which the warning is owed, from the obligation's lead time
    /// and the predicted impact.
    pub due_by: MissionTime,
    pub channel: String,
    pub state: WarningState,
    pub history: Vec<WarningTransition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WarningState {
    /// The obligation has triggered and nothing has been sent.
    Owed,
    /// Handed to the endpoint; not yet acknowledged.
    Sent,
    /// The receiving party acknowledged.
    Acknowledged,
    /// Sent after `due_by`, kept distinct so the measure can count it.
    Late,
    /// The endpoint refused or was unreachable. Never silently retried away.
    Failed { reason: String },
    /// A person judged it unnecessary and said why.
    Waived { operator: String, reason: String },
}
```

`Warning` is raised into the existing alert list, so PN-08 shows it without a new panel.

## 4. Edges

**One: `gungnir-workflow` to `gungnir-assessment`.**

`gungnir-workflow` depends on `gungnir-model`, `gungnir-security`, and
`gungnir-observability`. It needs `AssetExposure` and `ClosestApproach` to know when an
obligation triggers. The alternative was to have the binary compute the trigger and hand
`gungnir-workflow` a ready-made warning, which is the pattern D-13 chose for anomaly
detection.

The edge is taken instead, under the owner's rule of 2026-09-05, because the trigger is a
**rule about an obligation**, not a detector: it reads the obligation from the asset, the
prediction from assessment, and the state machine from workflow, and putting it in a
binary would put a policy rule in code that has no unit test and no second consumer
(AP-13). Acyclic: `gungnir-assessment` depends on `gungnir-model` only.

Recorded in [`dependency-edges.md`](dependency-edges.md).

## 5. Behaviour

The rule, evaluated on the tick:

1. For each asset with a `WarningObligation` and each track whose predicted impact or
   closest approach against that asset is inside `lead_time_s`, raise a `Warning` in state
   `Owed` if one is not already open for that pair.
2. Delivery is an **outbound endpoint call** through the generic-endpoint mechanism D-08
   settled. Success moves to `Sent`; the endpoint's acknowledgement, if it has one, moves
   to `Acknowledged`; a refusal or an unreachable endpoint moves to `Failed` with the
   reason, and the alert stays open.
3. A warning still `Owed` past `due_by` moves to `Late` and raises its severity. It is
   never closed by the passage of time.
4. A person may `Waive` with a reason. A waiver is a recorded decision with an operator on
   it, like every other.
5. When the track is deleted or its prediction no longer crosses the obligation, an open
   warning closes with its final state preserved in history. It does not disappear.

**What the system does not do:** it does not decide that a warning is unnecessary, and it
does not suppress a duplicate warning from a second track. Both are judgements, and AP-01
puts judgements with people. Deduplication is by asset-and-track pair only, which is
mechanical.

**Failure is loud.** An endpoint that cannot be reached leaves the warning visibly
`Failed`, raises an alert, and reports through `gungnir-observability`. A warning function
that quietly fails is worse than none, because the operator believes the asset was warned.

## 6. Configuration and interface delta

Lead time and channel come from the asset's `WarningObligation` (DN-01). The baseline gains
an endpoint table under D-08's generic-endpoint rule:
`ConfigBaseline.endpoints: Vec<EndpointConfig>` with a name, a kind, and an address.
`WarningObligation.channel` must name one, which DN-01's validation rule 5 already
enforces.

Interface: a new event variant. `Event` gains `Warning(WarningEvent)` with `Raised`,
`Sent`, `Acknowledged`, `Late`, `Failed`, and `Waived`. Clients treat unknown variants as
ignorable, which the compatibility rules already require.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-08 Alerts and incidents | Warnings appear as alerts with their state and time remaining; `Late` and `Failed` sort to the top |
| PN-04 Track detail | Warnings owed because of this track |
| PN-16 Planning panel | Which assets carry obligations, which is a laydown input |
| PN-17 Commander summary | Warnings owed, late, and failed in the current period |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-4.5 Warn assets and authorities | Replay with a configured obligation, plus a fault-injection test on the endpoint | A warning is raised no later than the obligation's lead time before predicted impact; an unreachable endpoint yields `Failed` with an alert and never a silent close; a warning past due becomes `Late` and stays open; every state change carries a mission time and, where a person acted, an operator | TT-02 and TT-04 sample sets; a stub endpoint that can be made to fail |

## 9. Amendment 1: the pass-close distance (2026-09-06, **signed by the owner the same day**)

§5 rule 1 says an obligation triggers when a track's "predicted impact **or closest
approach** against that asset is inside `lead_time_s`", and the obligation carried no
distance to judge a closest approach against: a surface craft passing a port a hundred
metres off never triggers, because it is never predicted to *impact*. `WarningObligation`
gains `within_m: Option<f64>`, and the baseline's asset gains `warning_within_m`.

The rule, restated: an obligation triggers for a track when

1. the track's time to impact is inside `lead_time_s` (unchanged), or
2. `within_m` is set and the track's predicted closest approach (DN-02's
   `closest_approach_m`) is at or inside it.

Impact is judged first, so a track that will arrive is reported by its time and not by
its distance. A pass-close warning is **due within `lead_time_s` of the prediction**
rather than at a time before the pass, because `AssetExposure` carries no time to closest
approach; when DN-02 supplies one, the due time moves to it by a further amendment and
the record already carries which trigger raised the warning (`warning::Trigger`).

Absent `within_m` means impact only, which is unchanged behaviour for every baseline
written before this amendment. Validation refuses a distance on an asset that has no lead
time and channel, and a non-positive one.

Verification row (§8) unchanged in criterion: "no later than the obligation's lead time
before predicted impact" still holds for rule 1, and the ledger test
`a_pass_inside_the_distance_triggers_and_moving_out_closes` covers rule 2.

## 10. Amendment 2: the pass-close due time (2026-09-06, **signed by the owner the same day**)

Amendment 1 owed a pass-close warning within `lead_time_s` of the prediction because
`AssetExposure` carried no time to closest approach. It carries one now
(`time_to_closest_approach_s`, from the same constant-velocity geometry as
`closest_approach_m`), so the due time moves to it: a pass-close warning is due by the
closest approach, and never later than the lead time -- `min(t_cpa, lead_time_s)`
after the prediction. `warning::Trigger::PassWithin` records the time it was raised
with, so the record says which rule set the due time. An exposure with no time (a track
that is not moving) keeps amendment 1's rule.

Verification row (§8) unchanged in criterion; the ledger test
`a_pass_close_warning_is_due_by_the_closest_approach` covers the due time.

## 11. Amendment 3: how an acknowledgement arrives (2026-09-06, **signed by the owner 2026-09-07**)

§5 rule 2 says a warning stands until the party acknowledges it. Until 2026-09-06 nothing
in the workspace could receive an acknowledgement: `WarningState::Acknowledged` existed,
`Warning::acknowledged` existed, and **no production code called either**. A delivered
warning sat in `Sent` and then went `Late`, for ever. The rule was written and unreachable.

The path, built the same day:

* `WarningEvent::Acknowledged { asset, track, party, at }`. Additive, so no
  `SCHEMA_VERSION` bump under §6's own compatibility rule. It carries **two times**: `at`
  is the party's claimed time and the envelope's `mission_time` is when this deployment
  recorded it, and a review that could not separate them could not tell a late
  acknowledgement from a late relay.
* `POST /v2/warnings/{asset_id}/{track_id}/acknowledge`, accepting either a machine whose
  certificate speaks for `MachineRole::WarnedParty { channel }` or an operator holding the
  new `warning.acknowledge` action. **`WarnedParty` is deliberately a separate role from
  `Effector`**: one certificate must not be able both to report an engagement and to
  discharge a warning.
* The node records and does not apply. It holds no warning ledger; the desktop that raised
  the warning applies it. That is the same split as the effector report.

**`Late` is acceptable to an acknowledgement, not only `Sent`.** `Late` means sent-or-owed
past the due time, and a party acknowledging after the deadline is precisely the case this
path exists for. `Owed` is still refused: nothing was sent, so there is nothing to
acknowledge.

**`warning.acknowledge` is held by the administrator alone**, matching `effector.report`.
Widening it to a supervisor or an operator keying in a radio acknowledgement wants a row
in `../mission/roles-and-stakeholders.md` §4 first.

## Traceability

GAP-042; CAP-4.5; D-08 for endpoints; depends on DN-01 for obligations and DN-02 for
predictions; `../ux/wireframes/WF-08-alerts-incidents.puml`; principles AP-01, AP-02,
AP-03.

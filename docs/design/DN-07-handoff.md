# DN-07 Effector handoff

Closes GAP-040. Status: first draft, 2026-09-05. **Design only; no code exists.**

## 1. The gap and the thread step it blocks

The contract as it stands has `POST /v1/detections` and `POST /v1/plans/{id}/decision`. Nothing
carries a decided assignment out to the system that will act on it. Step 7 of the
engagement sequence in MT-01, and the fires handoff in MT-06, are a radio call.

## 2. The owning component

`gungnir-api` for the message and the endpoint; `gungnir-model` for the payload type,
because the journal records it and `gungnir-remote` sends it.

`gungnir-api` depends on model, eventing, security, and both service facades. **No new
edge.**

## 3. Types

In `gungnir-model`:

```rust
/// What is handed to an effector or fires system after a human decided. One
/// shape for intercept and for fires (DN-05), because the receiving system needs
/// the same four things either way: what to act on, who decided, under what
/// authority, and where the target information came from.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Handoff {
    pub decision: DecisionId,
    pub plan: PlanId,
    pub kind: PlanKind,
    /// The decision as recorded: who, when, which role, which authority rule.
    pub decided_by: DecisionAttribution,
    /// Provenance of every track named in the plan, so the receiver can judge
    /// the quality of what it is being asked to act on.
    pub track_provenance: Vec<(TrackId, Provenance, Quality)>,
    /// Releasability marking (DN-17). A handoff crossing a boundary carries it.
    pub releasability: Releasability,
    pub issued: MissionTime,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecisionAttribution {
    pub operator: String,
    pub role: String,
    pub at: MissionTime,
    /// The authority rule that permitted it (DN-09), so the receiver can see
    /// the basis and an auditor can reconstruct it.
    pub authority_rule: Option<String>,
}

/// What the receiving system reports back. Drives DN-06's engagement states.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EffectorReport {
    Acknowledged { at: MissionTime },
    Executing { at: MissionTime },
    Completed { at: MissionTime, effective: bool, detail: String },
    Refused { at: MissionTime, reason: String },
}
```

## 4. Edges

**None.**

## 5. Behaviour

**A handoff is only ever produced from a recorded decision.** There is no path that
constructs one from a plan, and the constructor takes a `DecisionRecord` rather than a
`PlanView`. That is contract C-01 expressed in a type signature, and it is the reason the
attribution is not optional.

Delivery uses the generic-endpoint mechanism D-08 settled: each effector system is a
configured endpoint with a name and an address, and the handoff is posted to it. The
system does not need a bespoke integration per effector to be designed now, which is what
lets this land before any real agreement exists.

**States and failures:**

1. Delivery succeeds and the receiver acknowledges: DN-06 moves the engagement to
   `Executing` when the report says so.
2. The endpoint refuses: the handoff is recorded as refused with the reason, an alert is
   raised, and the engagement moves to `Aborted`. The decision itself stands; what failed
   is delivery.
3. The endpoint is unreachable: the handoff is queued by `gungnir-resilience`'s
   store-and-forward path, which already exists for detections and envelopes, and the
   panel shows it as undelivered. **It is never dropped and never presented as delivered.**
4. No endpoint is configured for the resource: the handoff is recorded and marked
   manual-delivery, which is the honest description of a radio call, and the operator sees
   that they must make it. This is the default state today and the design says so rather
   than failing.

Case 4 matters: it makes the system useful in a deployment with no integrated effector,
without ever claiming an automated handoff happened.

**Inbound reports** arrive on a new endpoint, are authenticated as the effector's machine
identity under D-02, and are journalled before they change any engagement state. A report
about an unknown decision is rejected and logged, not applied.

## 6. Configuration and interface delta

`ResourceConfig` gains `handoff_endpoint: Option<String>` naming an entry in the endpoint
table. Absent means manual delivery, which is case 4.

Interface, both additive:

| Method and path | Request | Response | Authorization action |
|---|---|---|---|
| `POST /v2/handoffs` | `Handoff` | `202`; a `HandoffEvent::Issued` appears on the stream | `plan.decide` |
| `POST /v2/handoffs/{decision_id}/report` | `EffectorReport` | `204` | new action `effector.report` |

`Event` gains `Handoff(HandoffEvent)` with `Issued`, `Delivered`, `Refused`, `Undelivered`,
and `Reported`.

The outbound direction, where the node posts a handoff to an effector rather than
receiving one, uses the same message shape against the configured endpoint. Defining one
shape for both directions is deliberate: two shapes would drift.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-05 Recommendation panel | Delivery state after a decision, with manual-delivery stated plainly when no endpoint exists |
| PN-06 Approval queue | An accepted item stays visible until its handoff is delivered or marked manual, so nothing falls between the decision and the act |
| PN-08 Alerts | Refused and undelivered handoffs |
| PN-20 Audit and accounts | Handoffs with their attribution, which is what an auditor asks for |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-4.4 Handoff with provenance | Type-level and unit tests with a stub effector endpoint | A handoff cannot be constructed without a decision record; every handoff carries an operator, a role, and a time; an unreachable endpoint queues rather than drops and is never shown as delivered; a report naming an unknown decision is rejected; no endpoint configured yields manual-delivery, not failure | Stub endpoint that can acknowledge, refuse, or hang; TT-01 replay |

The first criterion is checked by the compiler as well as by a test, which is the strongest
form available: there is no constructor taking a bare plan.

## Traceability

GAP-040; CAP-4.4; D-08 for endpoints, D-02 for the effector's identity; depends on DN-06
for engagement state, DN-05 for the fires variant, DN-17 for the marking;
`../gungnir-api-v1.md`; `../ux/wireframes/WF-05-recommendation.puml`; contracts C-01,
C-04, C-08.

# Interaction design: the critical flows

Status: first draft, 2026-09-04. Eight flows, each as a sequence or state diagram
(Mermaid, renders inline), the crate whose semantics it follows, and the gap that
implements what the code does not yet do. No flow introduces a state the crates do
not have; where a state is missing the diagram marks it and names the gap.

## FL-01 Alert: new to closed (`gungnir-workflow::AlertLifecycle`)

```mermaid
stateDiagram-v2
    [*] --> New : raised by observability (correlated incident)
    New --> Acknowledged : operator acknowledges (Ctrl+Shift+A or button)
    Acknowledged --> Escalated : operator or supervisor escalates with a note
    Acknowledged --> Closed : cause resolved
    Escalated --> Closed : supervisor or commander closes
    Closed --> [*]
    note right of New : strip count rises; row appears in PN-08; nothing pops over the queue
    note right of Escalated : commander's incident view; accepted-gap dialog offered when an asset is uncovered
```

Interaction rules: every transition records operator and mission time
(`AlertTransition`); invalid transitions are absent from the UI, not greyed; bulk
acknowledgement is allowed only for Info severity; a Critical incident opens its
history on selection so the raw alerts are one click away.

## FL-02 Plan: proposed to decided (`gungnir-policy`, `gungnir-command`, `gungnir-model::events`)

```mermaid
sequenceDiagram
    participant IS as Intercept service (SV-02)
    participant PO as Policy chain (SV-15)
    participant AW as Approval workflow (SV-16)
    participant Q as PN-06 Queue
    participant R as PN-05 Recommendation
    participant D as PN-07 Decision dialog
    participant OP as Operator (P-01)
    participant AU as Audit (SV-22)
    IS->>PO: PlanProposed (PlanView)
    PO-->>Q: PolicyVerdict Denied {reason} (row shown as denied, no dialog)
    PO->>AW: RequiresHumanApproval (or Approved under pre-delegation, D-15)
    AW->>Q: ApprovalRequested (row with priority, time remaining, assignee)
    OP->>Q: select row (Enter)
    Q->>R: show plan, verdict, rationale, alternatives, cost, time remaining
    OP->>R: open decision…
    R->>D: modal with the plan as shown; delegation check; degraded acknowledgement if stale
    alt accept
        OP->>D: Accept (last in tab order, never Enter)
        D->>AW: decide(Accepted)
    else override
        OP->>D: choose substitute; policy re-checks it; reason required
        D->>AW: decide(Overridden, substitute plan)
    else reject
        OP->>D: reason required
        D->>AW: decide(Rejected)
    end
    AW->>AU: AuditEntry (GAP-059)
    AW-->>Q: CommandEvent::Decided; row shows the decision and who
    AW-->>IS: PlanApproved when actionable; handoff via SV-23 (GAP-040)
    IS-->>D: PlanSuperseded while open → Accept disabled, "plan changed, reopen"
```

Missing states named in the diagram: expiry and escalation of a pending approval
(GAP-034), queue ordering and pre-delegation marks (GAP-035), the wiring of the
policy chain and workflow into the tick (GAP-028), the panel itself (GAP-038).

## FL-03 Replay scrubbing (`gungnir-replay::ReplaySession`)

```mermaid
sequenceDiagram
    participant A as Analyst (P-03)
    participant P as PN-12 Replay
    participant RS as ReplaySession
    participant V as PN-02 Viewport
    A->>P: open session 1757030000
    P->>RS: open(journal, session)
    RS-->>P: len, first and last mission time, clock (replay)
    P-->>V: strip says Replaying; viewport watermark
    A->>P: drag the scrubber to 01:41:02
    P->>RS: seek_to(01:41:02)
    RS-->>P: position; the picture is rebuilt from envelopes up to that time
    P-->>V: tracks and plan as of 01:41:02
    A->>P: step ▶
    P->>RS: step()
    RS-->>P: envelope seq 18402 (ApprovalRequested P-1183)
    P-->>V: apply the one event; highlight P-1183
    A->>P: play at 4×
    loop each tick at 4× replay clock
        P->>RS: step() while envelope time ≤ replay clock
    end
    A->>P: add annotation on T-042
    P->>P: Case "VG-01 review" gains an Annotation (author, time, text, track)
```

Rule: replay never writes to the live journal; annotations live in the case. A gap
in `seq` shows a banner. Deterministic replay (MOP-15) is what makes the scrubbed
picture trustworthy; the rehearsal variant that runs a scenario through the live
pipeline is GAP-045.

## FL-04 Sensor mode change (`gungnir-sensor-management::SensorMode`, `gungnir-analytics`)

```mermaid
sequenceDiagram
    participant SM as Sensor manager (P-04)
    participant S as PN-10 Sensors
    participant C as PN-11 Coverage
    participant REG as SensorRegistry (SV-09)
    participant AN as Analytics (SV-14)
    participant OUT as Outbound control (GAP-004)
    participant AU as Audit (SV-22)
    SM->>S: select R2, choose "search" (only can_transition_to modes offered)
    S->>AN: coverage for current mode and for "search"
    AN-->>C: before and after side by side; gaps 1 → 0 (MOP-21 under 10 s)
    SM->>S: reason "cover upper Vell after R1 loss"; Commit (enabled only after before/after shown)
    S->>REG: set mode (refuses invalid transitions with InvalidModeTransition)
    REG->>AU: AuditEntry sensor.task (GAP-059)
    REG->>OUT: control message to the sensor's adapter (GAP-004)
    OUT-->>S: acknowledged or timed out; mode shown as "requested" until reported back
    REG-->>C: coverage recomputed; strip health unchanged
```

Rule: the panel shows requested versus reported mode as two states so a change
that never reached the sensor is visible.

## FL-05 Disconnected fallback and reconnection (`gungnir-remote`, `gungnir-resilience`, `gungnir-collab`)

```mermaid
stateDiagram-v2
    [*] --> Connected : Remote backend, node reachable
    Connected --> Detached : heartbeat lost (GAP-050) or connect failed at startup (today)
    Detached --> Detached : operate on embedded services under the delegation in force (D-15); outbox and queue fill; strip shows counts
    Detached --> Reconciling : link returns; outbox drains; journals merged (reconcile)
    Reconciling --> Conflicts : ReconciliationReport has conflicts → PN-18 opens for the supervisor
    Conflicts --> Connected : supervisor confirms arbitration or overturns with a reason
    Reconciling --> Connected : no conflicts
    note right of Detached : delegation expires after the configured interval; the strip counts down; new delegation impossible while detached
    note right of Conflicts : nothing is written to the node's record until decided; Escape does not close PN-18
```

The operator's side: the strip chip turns "Detached, N queued, M dropped", the
alert lifecycle raises one incident, the queue keeps working locally. The
supervisor's side: PN-18 as in WF-18. Today only the startup fallback exists; the
heartbeat and mid-session switch are GAP-050 and the transport GAP-041.

## FL-06 Assistant question and answer (plan 08, `gungnir-agent`)

```mermaid
sequenceDiagram
    participant OP as Operator (P-01)
    participant A as PN-19 Assistant
    participant AG as gungnir-agent
    participant T as Read-only tools over the snapshot and journal
    participant LLM as Provider (cloud or local per D-14)
    participant AU as Audit (SV-22)
    OP->>A: "why is T-042 scored 87?"
    A->>AG: question + role + selected track
    AG->>T: get_track(T-042), get_score_factors(T-042)
    T-->>AG: data with snapshot time
    AG->>LLM: context assembled under the egress policy for this profile
    LLM-->>AG: answer
    AG->>AU: AuditEntry of the exchange
    AG-->>A: answer with provenance (model, tools, snapshot time); DRAFT label on any draft
    OP->>A: "show the factors in PN-04" (a navigation, not a state change)
```

Rules: no control in PN-19 changes state; drafts are labelled and copied only into
case notes; when no provider is reachable the panel says so. Evaluation and
injection resistance are MOP-35.

## FL-07 Identity declaration (`gungnir-identification`, evidence card)

```mermaid
sequenceDiagram
    participant OP as Operator or intelligence analyst
    participant E as PN-04 Evidence card
    participant ID as IdentificationEngine (SV-12)
    participant PO as Policy (per-class criteria, GAP-018)
    participant AU as Audit
    OP->>E: select T-044 (unknown, 0.41)
    E-->>OP: every evidence item with source, weight, time; margin for the class
    OP->>E: designate identity…
    E->>PO: class criteria and whether a person may declare
    alt below the margin
        E-->>OP: refused with the margin shown; "escalate to supervisor"
    else at or above the margin
        OP->>E: declare hostile with a note
        E->>ID: designation added as evidence with the operator's identity
        ID-->>E: classification recomputed; confidence shown
        E->>AU: AuditEntry
    end
```

## FL-08 Weapons control status change (policy configuration, supervisor)

```mermaid
sequenceDiagram
    participant SUP as Supervisor (P-02)
    participant ST as PN-01 Strip
    participant DG as Status dialog
    participant PO as Policy (GAP-033, GAP-052)
    participant Q as PN-06 Queue
    participant AU as Audit
    SUP->>ST: change WCS…
    ST->>DG: current status per layer; the plans each change would affect
    SUP->>DG: area layer Tight → Hold; reason
    DG->>PO: set status; re-evaluate pending plans
    PO-->>Q: plans on the area layer become Denied {status hold}; rows update; no dialog pops
    PO->>AU: AuditEntry wcs.set
    ST-->>SUP: chip shows Hold with who and when on every layout
```

## Cross-flow rules

- A decision dialog acts on exactly one item (principle 8) and shows time remaining
  (principle 9).
- Nothing pops over the queue; new items arrive as rows and strip counts.
- Every flow that writes ends in an audit entry; the one that does not today
  (GAP-059) is marked in the diagrams.
- Every flow's crate is named so the interaction cannot drift from the semantics.

## Traceability

- Task analyses: FL-01 T-op-5, T-sup-3; FL-02 T-op-3, T-op-4, T-sup-1, T-cd-6;
  FL-03 T-an-1, T-pl-4; FL-04 T-sm-2; FL-05 T-op-7, T-sup-5; FL-06 T-an-4 and the
  assistant surfaces; FL-07 T-op-2, T-ia-2; FL-08 T-sup-2.
- UAF: Op-St (the same machines), Sv-Pr (the decision path), Sc-Pr (gating and
  audit).
- Code map: `ux-to-code-map.md`.

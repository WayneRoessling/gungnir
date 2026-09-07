# DN-21 Battle rhythm

Closes GAP-054. Status: first draft 2026-09-05; **implemented 2026-09-05**, see
amendment 1 for the three places the implementation departed from this note.

The line that used to stand here said "design only; no code exists". It was already
wrong when GAP-054 was picked up: `gungnir-reporting/src/rhythm.rs` held the types and
their tests, and **nothing constructed any of them** -- no schedule was read from a
baseline, no window reached a sensor record, no handover was ever assembled.

## 1. The gap and the thread step it blocks

A watch runs on a rhythm: shifts hand over, reports go out on a cycle, sensors come down
for maintenance on a schedule. None of it is supported, so handover depends on memory and
MOE-13 cannot be met.

## 2. The owning components

Two, because the gap is really two things:

| Concern | Crate |
|---|---|
| Scheduled products and handover summaries | `gungnir-reporting` |
| Maintenance windows | `gungnir-sensor-management` |

Both already have what they need. **No new edge.**

## 3. Types

In `gungnir-reporting`:

```rust
/// A product the deployment produces on a cycle rather than on demand.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScheduledProduct {
    pub name: String,
    pub kind: ProductKind,
    pub schedule: Schedule,
    pub releasability: Releasability,
    /// Endpoint to deliver to, if it goes anywhere. Absent means it is produced
    /// and held for a person to read.
    pub deliver_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProductKind {
    /// What happened on this watch, for the people taking over.
    HandoverSummary,
    /// Periodic situation report.
    SituationReport,
    /// Measures for the period.
    MeasuresSummary,
}

/// Deliberately simple: a period and an offset, in mission time. Not a cron
/// expression, because a rhythm nobody can read in the panel is a rhythm nobody
/// checks.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Schedule {
    pub period_s: f64,
    pub offset_s: f64,
}

/// A handover summary: what the incoming watch needs, assembled from the record.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandoverSummary {
    pub period: (MissionTime, MissionTime),
    pub outgoing: Option<String>,
    pub open_alerts: Vec<AlertRef>,
    pub open_engagements: Vec<DecisionId>,
    pub pending_approvals: usize,
    pub expired_approvals: usize,
    pub sensors_degraded: Vec<SensorId>,
    pub maintenance_due: Vec<MaintenanceWindow>,
    pub warnings_owed: Vec<AssetId>,
    pub baseline_version: u32,
    /// Free text the outgoing watch adds. The one part the system does not
    /// assemble, and the part that matters most.
    pub notes: Option<String>,
    pub acknowledged_by: Option<String>,
}
```

In `gungnir-sensor-management`:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MaintenanceWindow {
    pub sensor: SensorId,
    pub from: MissionTime,
    pub to: MissionTime,
    pub reason: String,
    pub state: MaintenanceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MaintenanceState {
    Planned,
    /// The window is open and the sensor is expected to be down.
    Active,
    Completed,
    /// The window passed without the sensor going down or coming back.
    Overrun,
}

impl SensorRecord {
    /// True when this sensor is offline inside a planned window. Distinguishes
    /// expected absence from failure, which is the whole point.
    pub fn is_in_maintenance(&self, now: MissionTime) -> bool;
}
```

## 4. Edges

**None.**

## 5. Behaviour

**The handover summary is assembled from the record and completed by a person.** Every
field except `notes` comes from state the system already holds. `notes` is the outgoing
watch's judgement, and the summary is not complete without an acknowledgement from the
incoming watch, recorded with a name and a time.

That acknowledgement is the measure. MOE-13 counts handovers acknowledged inside the
window, and an unacknowledged handover is visible rather than assumed.

**Maintenance changes what a degraded sensor means.** Today a sensor going offline raises a
health alert. Inside a planned window it does not: it is expected, the health panel shows
it as in maintenance, and coverage (DN-12) reports the resulting gap as planned rather than
as a loss. A sensor that fails to return when its window closes becomes `Overrun` and
raises an alert, which is the case that actually needs attention.

**A maintenance window never suppresses a coverage gap.** The gap is real whether it was
planned or not, and a planner comparing laydowns needs to see it. Only the alert changes,
not the picture. Confusing "expected" with "not a problem" is how a scheduled outage
becomes an unnoticed hole.

**Scheduled products are produced, not sent silently.** A product delivered to an endpoint
follows DN-07's delivery rules: queued when unreachable, never dropped, visible when
undelivered. A product with no endpoint is produced and held.

**The scheduler runs on mission time**, so replay reproduces the same products. A wall-clock
scheduler would make a replayed session produce a different set, which breaks AP-08.

## 6. Configuration and interface delta

`ConfigBaseline.reporting.scheduled: Vec<ScheduledProduct>`, validated so that periods are
positive, offsets are less than periods, and endpoints exist.

`ConfigBaseline.sensors` gains `maintenance: Vec<MaintenanceWindowConfig>`, validated so
that windows do not overlap for one sensor and `to` is after `from`.

Interface, additive:

| Method and path | Purpose | Authorization action |
|---|---|---|
| `GET /v2/handover` | The current period's summary | `picture.view` |
| `POST /v2/handover/acknowledge` | Record the incoming watch | new action `handover.acknowledge` |

`Event` gains `Handover(HandoverEvent)` with `Produced` and `Acknowledged`, and
`SensorTaskEvent` gains `MaintenanceOverrun`.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-17 Commander summary | The handover summary is this panel's content at shift change, with the notes field editable by the outgoing watch |
| PN-09 System health | Sensors in maintenance shown distinctly from sensors failed; overruns highlighted |
| PN-11 Coverage layer controls | Planned gaps marked as planned, still drawn as gaps |
| PN-13 Reports | Scheduled products with their next due time and their delivery state |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-5.8 Battle rhythm | Replay across a scheduled boundary, plus unit tests on the window state machine | A replayed session produces the same scheduled products at the same mission times; a sensor offline inside a window raises no failure alert but still contributes a coverage gap; a sensor not returning by the window's end becomes `Overrun` and alerts; a handover is incomplete until acknowledged with a name and a time | TT-07 sample set replayed across a boundary with a configured window |

The first criterion is the one that proves the scheduler is on mission time, and it is
worth a dedicated test because a wall-clock implementation would pass every other check.

## 9. Amendment 1 -- **signed by the owner 2026-09-05**

Raised by GAP-054 on 2026-09-05, by implementing this note. Three departures, each because
the note's placement could not be built as written. The same sign-off covers the code that
conforms to it.

**(a) The shared types are in `gungnir-model`, not in one consumer.** §3 puts
`MaintenanceWindow` in `gungnir-sensor-management`; the first draft of the code put it,
`Schedule` and `ScheduledProduct` in `gungnir-reporting`. Four crates need them: the
registry (is this absence expected?), `gungnir-reporting` (the summary), `gungnir-config`
(loading one), and the binary (what a silent sensor means). **Either placement forces an
edge for the other**, and §4 says this design adds none -- which is only true if the types
live where every layer already looks. `gungnir-reporting` re-exports them, so nothing that
used them moved. `HandoverSummary` stays in `gungnir-reporting`: it is assembled from the
record, which is genuinely that crate's job.

**(b) One `Event::Rhythm(RhythmEvent)` rather than `Event::Handover` plus a
`SensorTaskEvent` variant.** §6 asks for the overrun on `SensorTaskEvent`. It does not fit:
**every variant of that enum carries a `SensorTaskId`** and `SensorTaskEvent::task()`
returns one unconditionally, because a task event without a task is meaningless. A
maintenance window is not a task -- nobody asked a sensor for anything -- so putting it
there would have made that accessor fallible for every caller in order to serve one
variant. One rhythm variant keeps the note's three kinds together and leaves tasking's
invariant intact.

**(c) A product owed to an endpoint is recorded as undelivered.** §5 says delivery follows
DN-07's rules: queued when unreachable, never dropped, visible when undelivered. There is
no delivery path at all yet (GAP-040), so `RhythmEvent::ProductUndelivered` names the
product, the endpoint and the reason on every cycle. **A deployment that believed it was
reporting to higher command and was not** is precisely what that rule exists to prevent,
and a silent no-op would have produced it.

Two smaller decisions worth recording. The first tick establishes the scheduler's mark
rather than firing every product whose schedule ever passed -- on a session whose clock
starts at Unix seconds that would be decades of handover summaries in one frame. And a
maintenance overrun alerts **once**, not every frame: an alert repeated every tick is an
alert nobody reads.

Still open after this amendment: §7's PN-11 row (planned gaps marked as planned, still
drawn) and PN-13 row (scheduled products with their next due time), the `MeasuresSummary`
product's *content*, which is GAP-047, the assistant drafting the notes field (GAP-044),
the two `/v2/handover` endpoints in §6, and §8's TT-07 replay, which needs GAP-046 -- the
four criteria are demonstrated against a generated baseline and the desktop's own tick
instead, and the verification row says so rather than being widened.

## Traceability

GAP-054; CAP-5.8; MOE-13; depends on GAP-047 for measures, DN-07 for delivery, DN-12 for
coverage, DN-17 for markings; the assistant drafts the notes field under
`../ai/concept-of-operations.md`; `../ux/wireframes/WF-17-commander-summary.puml`,
`WF-09-system-health.puml`; principles AP-02, AP-08.

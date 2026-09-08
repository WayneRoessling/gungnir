# DN-11 Sensor control and tasking

Closes GAP-004 and GAP-005. Status: first draft, 2026-09-05. One note for two gaps,
because they are the same control path in opposite directions: a request coming in and a
command going out.

**Implementation status, 2026-09-05.** §3's sensor-management types, §5's outbound
behaviour, §6's configuration delta and `Event` variant, and §7's PN-10 row are built,
and §8's first row is exercised by unit tests against a stub adapter.

The requirements half followed the same day (GAP-005): §3's `CollectionRequirement`,
§4's edge, §5's **Requirements** paragraph, and §7's PN-15 row are built, and the model
gained the `priority` field §3 shows and this note's implementation had omitted.

Where the implementation departs from this note, §9 and §10 record it: amendment 1,
signed by the owner on 2026-09-05, and amendment 2 (the first `SensorControlAdapter`),
signed by the owner on 2026-09-07.

What is **not** built: the delivery itself, which needs an adapter (GAP-001); the
`POST /v2/sensors/{sensor_id}/task` endpoint, which needs the transport (GAP-041);
persistence of the requirement list across restarts; and §8's second verification row,
whose criterion cannot be met by any build without an operator session. §7's PN-04 camera
cue is deferred: a cue control for a sensor nothing can cue would be the appearance of
capability that rule 4 exists to prevent.

## 1. The gap and the thread steps it blocks

**GAP-004.** Mode changes and tasking are recorded locally and nothing reaches a sensor.
MT-07 recovery and MT-03 camera cueing stop at the operator's screen.

**GAP-005.** MT-08 collection management needs requirement objects, a tasking request from
the intelligence analyst, and the sensor manager's concurrence. Only modes and coverage
exist, so MT-08 steps 2 and 3 happen outside the system.

## 2. The owning components

| Concern | Crate |
|---|---|
| The outbound control message and its acknowledgement | `gungnir-sensor-management` |
| The requirement object and the tasking case | `gungnir-workflow` |
| The types both carry | `gungnir-model` |

`gungnir-sensor-management` depends on `gungnir-coord`, `gungnir-model`, and
`gungnir-config`, which is everything the outbound path needs.

## 3. Types

In `gungnir-model`:

```rust
/// A collection requirement: what somebody needs to know, independent of which
/// sensor answers it. Owned by the intelligence analyst (MT-08).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CollectionRequirement {
    pub id: RequirementId,
    pub title: String,
    pub priority: AssetPriority,
    pub area: AssetExtent,
    pub needed_by: Option<MissionTime>,
    pub state: RequirementState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RequirementState {
    /// Stated, not yet matched to a sensor task.
    Stated,
    /// A sensor task exists and the sensor manager concurred.
    Tasked,
    /// Answered, with the evidence referenced.
    Satisfied,
    /// The sensor manager declined, with a reason.
    Declined,
    /// The time passed without an answer.
    Lapsed,
}
```

In `gungnir-sensor-management`:

```rust
/// A command to a sensor. What it means on the wire is the adapter's business;
/// this is the intent the registry records and the audit trail keeps.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SensorCommand {
    SetMode { mode: SensorMode },
    /// Point at a place, for a camera or a directional sensor.
    Cue { target: Geodetic, dwell_s: Option<f64> },
    /// Search a region until told otherwise.
    Search { area: AssetExtent },
    Calibrate { procedure: String },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorTask {
    pub id: SensorTaskId,
    pub sensor: SensorId,
    pub command: SensorCommand,
    /// The requirement this serves, when it came from one.
    pub requirement: Option<RequirementId>,
    pub issued: MissionTime,
    pub state: TaskState,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaskState {
    /// Recorded here; not yet handed to the adapter.
    Issued,
    /// The adapter accepted it for delivery.
    Sent,
    /// The sensor acknowledged.
    Acknowledged { at: MissionTime },
    /// The sensor refused, or the adapter could not deliver.
    Failed { reason: String },
    /// No acknowledgement inside the window.
    Unacknowledged,
}

/// Outbound counterpart of the ingest adapter. One implementation per sensor
/// interface agreement; none exists until GAP-001 brings the adapters.
pub trait SensorControlAdapter: Send + Sync {
    fn issue(&self, task: &SensorTask) -> Result<(), SensorManagementError>;
}
```

## 4. Edges

**One: `gungnir-workflow` to `gungnir-sensor-management`.**

The tasking case links a requirement to the sensor tasks that serve it and shows their
state. The alternative was to link by identifier only, with the binary joining the two,
and the plan's edge table flagged this as the case most likely to be avoidable that way.

On reflection the edge is taken, but narrowly: `gungnir-workflow` reads `SensorTask` and
`TaskState` and never issues a command. Issuing stays with the registry, so the crate that
can talk to a sensor is still exactly one crate. Acyclic:
`gungnir-sensor-management` depends on coord, model, and config, none of which is workflow.

Recorded in [`dependency-edges.md`](dependency-edges.md), flagged for the engineering
reviewer as the weakest of the six.

## 5. Behaviour

**Outbound.** `SensorRegistry::set_mode` today changes local state. It gains a companion
that records a `SensorTask`, hands it to the adapter, and tracks the acknowledgement.

The rules that keep it honest:

1. **Local state does not change until the sensor acknowledges.** A mode the operator asked
   for and the sensor never took is displayed as requested-not-confirmed, not as the
   current mode. This is the single most important rule in the note: a registry that
   reports the mode it asked for is a health flag that lies (AP-02).
2. An unacknowledged task past its window becomes `Unacknowledged` and raises an alert.
3. A failure carries the adapter's reason and never retries silently. Retry is an operator
   action.
4. With no adapter configured for a sensor, which is every sensor today, issuing returns
   `NotImplemented` and the panel says the sensor is not controllable from here. It does
   not appear to succeed.

Rule 4 is what lets this design land before GAP-001 without pretending.

**Requirements.** The workflow: the intelligence analyst states a requirement; the sensor
manager tasks it, declines it with a reason, or it lapses; satisfaction references the
evidence that answered it. Concurrence is an authorized action, `sensor.task`, which
already exists in `gungnir_security::actions`, so the authority matrix governs it without
a new action name.

A requirement is never satisfied automatically. A task acknowledged is not an answer, and
inferring one would be the same error as inferring effect from a track deletion (DN-06).

## 6. Configuration and interface delta

`ConfigBaseline.sensors` gains `control_endpoint: Option<String>` naming an entry in the
endpoint table DN-03 introduces. Absent means not controllable, which is rule 4.

`ConfigBaseline.sensor_task_ack_window_s: f64`, defaulted and validated positive.

Interface:

- `POST /v2/sensors/{sensor_id}/task` with `SensorCommand`, authorization action
  `sensor.task`, returning the task identifier. New endpoint, additive.
- `Event` gains `SensorTask(SensorTaskEvent)` with `Issued`, `Acknowledged`, `Failed`,
  `Unacknowledged`.
- `SnapshotResponse` gains `requirements: Vec<CollectionRequirement>`.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-10 Sensor management | Task state per sensor, requested-versus-confirmed mode shown as two distinct things, and a control that is disabled with a reason when no adapter is configured |
| PN-15 Requirements and tasking | The requirement list with state, the tasks serving each, and the concurrence action. This panel exists in the information architecture and is blocked on GAP-005 |
| PN-08 Alerts | Unacknowledged and failed tasks |
| PN-04 Track detail | A cue control for a camera, when one is controllable |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-1.3 Sensor modes and tasking | Unit tests with a stub adapter that can acknowledge, refuse, or ignore | Local mode never changes before acknowledgement; an ignored task becomes `Unacknowledged` inside the window and alerts; a sensor with no control endpoint returns a not-implemented error and no state changes; no automatic retry occurs | Stub adapter; generated sensor sets |
| CAP-2.12 Pattern of life and order of battle, tasking part | MT-08 replay | A requirement moves Stated to Tasked only with a concurrence carrying an operator; satisfaction always references evidence; a requirement past `needed_by` lapses rather than remaining open | TT-08 sample set |

## 9. Amendment 1 -- **signed by the owner 2026-09-05**

Raised when GAP-004 and GAP-005 implemented this note. Each item is a correction of the
note against what implementing it showed. The same sign-off covers the code that conforms
to it.

**a. `Event` gains `Requirement(RequirementEvent)`.** §6 lists the interface delta and the
`SensorTask` event, and no requirement event. Without one the requirement lifecycle exists
only in memory -- and §8's CAP-2.12 method is an MT-08 **replay**, which can read nothing
but the journal. A concurrence that names who concurred and never reaches the journal
cannot be reviewed, which is the whole reason §5 requires it to name anybody. The variants
are `Stated`, `Tasked`, `Declined`, `Satisfied`, `Lapsed`.

`Tasked` carries the `SensorTaskId`, so the link §4 draws between a requirement and the
tasks serving it survives into the record instead of living only in the panel. `Lapsed` is
its own variant rather than a `Declined` with an empty reason, for the reason DN-10 keeps
an expiry distinct from a rejection: nobody refused a lapsed requirement.

**b. `RequirementState::Tasked` and `Declined` carry a `Concurrence`, not a `String`.**
§5 requires a concurrence to carry an operator, and no build has an operator session --
D-02's signed tokens are GAP-057. A name that was simply absent would be ambiguous between
"nobody is signed in" and "we failed to record who", which are different facts about the
same record and only one of them is a defect. `Concurrence::Operator` is what §8's CAP-2.12
criterion asks for; `Concurrence::UnattributedRole` is what this system can produce today,
and it records the role while stating that nobody was signed in rather than putting a role
name in a field an auditor reads as a person.

**The CAP-2.12 row in §8 is unchanged.** It asks for a concurrence carrying an operator,
and no build can satisfy that until GAP-057. Widening it to accept a role would have made
it pass by describing what was built rather than what is required.

**c. A requirement moves to `Tasked` only when a task exists.** §3 defines the state as a
sensor task existing *and* the sensor manager concurring, and the first implementation
checked only the second half -- so a requirement could sit in `Tasked` with nothing serving
it, which the analyst who stated it would read as work in hand.
`gungnir_workflow::TaskingCase::concur` now refuses without a task, and PN-15's control
issues the command first, recording the concurrence only if a task really came out of it.

**d. A decline requires a reason.** §5 says the sensor manager "declines it with a reason";
nothing enforced it. The analyst has to know whether to restate the requirement differently
or give up on it, and "declined" alone answers neither question. Same rule PN-07 applies to
rejecting a plan.

**e. `CollectionRequirement` gains the `priority` field §3 already showed.** The first
implementation omitted it. It shares `AssetPriority` with defended assets rather than
introducing a parallel scale.

**f. `SensorControl::issue` takes the requirement as a required argument.** `SensorTask`
always had the field and nothing ever set it, so no requirement could be linked to the
tasks serving it however hard `gungnir-workflow` tried. A convenience overload would have
let that recur silently; making every caller say `None` deliberately is what stops it.

**g. `SensorControlAdapter` is attachable, and `TaskState::Sent` means one accepted the
task.** §3 declared the trait and nothing could hold an implementation of it, so §8's
stated method -- unit tests against a stub that can acknowledge, refuse, or ignore -- had
no seam to plug into. `InMemorySensorRegistry::attach_adapter` is that seam. Nothing
attaches one outside tests: GAP-001 brings the adapters, and until then every task stops
at `Issued` and `has_adapter()` reports false rather than leaving it to be inferred.

## 10. Amendment 2 (2026-09-07): the first `SensorControlAdapter`, and what it cannot say

**Signed by the owner 2026-09-07.**

Item g of amendment 1 built the seam and said "GAP-001 brings the adapters." One does now:
`gungnir_sensor_management::sapient_task::SapientTaskAdapter`, the outbound counterpart of
`gungnir-ingest`'s SAPIENT spotter adapter, both pinned against the same ICD
(`docs/design/external-standards.md` §7).

**No new edge.** Building the JSON `Task` message needs `serde_json`, already a workspace
dependency, and nothing `gungnir-ingest` owns; putting it in `gungnir-sensor-management`,
where `SensorControlAdapter` is defined, means the edge §4 already argued stays the only
one.

**Three of `SensorCommand`'s four variants map cleanly onto SAPIENT's `Task.command`; the
fourth does not, and is refused rather than guessed.** `Cue` becomes a single-point
`LocationList` under `look_at`, since SAPIENT's `LocationOrRangeBearing` has no dedicated
point variant and a `RangeBearingCone` would need the sensor's own position, which this
crate does not hold. `Calibrate` becomes the `oneof`'s free-text `request`. `SetMode`
becomes `mode_change`, also free text -- the ICD leaves ASM mode names to the deployment,
so this adapter states its own and any receiver has to agree on them out of band regardless
of what is chosen. `Search` **is refused**: a search area could be sent as a `Region`, but
a `Region` also carries classification and behaviour filters `AssetExtent` says nothing
about, and a `Task` with an empty filter list is not obviously the same request as "search
this area." That is an interface agreement nobody has made, and this note does not make it
either -- the adapter names the reason and refuses rather than sending a `Task` whose
meaning would be guessed, the same discipline DN-27 §2 applies to a bearing.

**Delivery, not transport, and the round trip is not built.** `issue` builds a
well-formed `Task` and hands the JSON text to an injected sink; what carries it to a real
SAPIENT node from there is the same open row the inbound adapter already carries; decoding
binary protobuf needs a runtime this workspace has not admitted under §2.9. `TaskAck` is
not parsed, so `SensorControl::acknowledge` -- item g's seam -- has no caller from this
adapter yet. GAP-004's own closing action is a command reaching a sensor "over a specified
interface rather than stopping at the node," which this settles; the acknowledgement
reaching back is recorded as the next item rather than attempted here.

**A `task_id` and a message `timestamp`, without a `ulid` or `chrono` dependency.** The
ICD's `task_id` is annotated `is_ulid: true` and the envelope's `timestamp` needs an RFC
3339 UTC instant; both are built by hand against public, well-known algorithms (a 48-bit
timestamp plus 80-bit payload in Crockford base32; Howard Hinnant's `civil_from_days`, the
inbound adapter's own `days_from_civil` run backwards) rather than by adding a crate for
either. The encoder is checked against the published ULID specification's own worked
example.

## Traceability

GAP-004, GAP-005; CAP-1.3, CAP-2.12; D-08 for the endpoint model; depends on GAP-001 for
real adapters and DN-01 for `AssetExtent`;
`../ux/wireframes/WF-10-sensor-management.puml`, `WF-15-requirements.puml`; principles
AP-02, AP-03.

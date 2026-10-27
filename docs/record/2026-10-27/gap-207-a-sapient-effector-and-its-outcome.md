# GAP-207 a SAPIENT effector and its outcome without a node

Plan 12 tranche A, the fifth and last build. The review found that no adapter spoke to a
jammer or an interceptor. A handoff was a generic HTTP post or a radio call. The effector
report route lives on the node and the desktop binds no listener, so in the disconnected
profile no outcome ever came back by data. PN-05 reminded the operator to make a radio call
and never learned what happened. D-144 put the trial's one effector on SAPIENT until a
partner names its own interface.

## The owner's four choices, 2026-10-27

Each was the recommended default.

| Question | Chosen |
|---|---|
| A handoff as a SAPIENT `Task` | `command.request = "engage"`, with one area-of-interest region per target at its solved intercept point. No point solved means the handoff is refused by name, not sent |
| `TaskAck` `COMPLETED`, which says nothing about effect | A new `EffectorReport::Finished`. The engagement stays open and closes on the track's evidence |
| How a resource names the effector | An endpoint of kind `sapient` and one `sapient_effectors` row |
| Which binaries | Both |

## What SAPIENT has, read from the protobufs

BSI Flex 335 v2.0 has five effector node types, from kinetic to RF directed energy. It has
no engage command and no field for an effect. So `engage` is our vocabulary, documented as
the sensor mode names already are, and `COMPLETED` cannot honestly be read as effective or
ineffective. `docs/design/external-standards.md` §7.4 quotes the protobufs.

## What was built

- **`gungnir-model`.** `EffectorReport::Finished { at, detail }`: the task is over and the
  effect is not stated. It is additive on the wire, so the node's report route accepts it
  too.
- **`gungnir-sensor-management`.** `engage_task` builds the `Task`, and `engage_payload`
  builds it from the baseline. It is deterministic: a retry carries the same task id. It
  refuses a fires task, a plan tasking none of this effector's resources, and a target with
  no intercept point.
- **`gungnir-ingest`.** `SapientEffectorLink` opens the connection, writes tasks and reads
  `TaskAck`s. A `TaskAck` is taken only from the configured node id, for a task this link
  sent. It is refused from a node that registered as another type. A task is delivered on
  its first `TaskAck`, and unreachable if the write failed or ten seconds pass without one.
  `effector_report` maps each status. `SapientEffectors` is the per-host registry with the
  task-to-decision table. `TcpTaskSink::try_send` now says whether a write went out.
- **`gungnir-approval` (the decision path, human-owned under D-65).** `HandoffTransport`
  gains a defaulted `payload_for`, so a host says what an endpoint is sent. Every HTTP
  endpoint is sent exactly what it was. `apply_report` handles `Finished` by leaving the
  engagement open and saying why.
- **`gungnir-config`.** The `sapient` endpoint kind and `sapient_effectors` rows, validated
  together. The section applies as a Deployment section.
- **Both binaries.** Each binds its effectors at start, connecting if the middleware
  answers and otherwise reconnecting on the next handoff. Each answers the transport's
  three questions for a `sapient` endpoint. Each sweep reads the effectors before the
  handoff sweep and applies their reports after it.
- **Documents.** DN-07 §10 (amendment 2), external-standards §7.4, a deployment README
  section "A SAPIENT effector", and the roles table.

## Tests

- **`gungnir-app/tests/sapient_effector.rs`.** A desktop with no node and a jammer behind a
  loopback middleware. The decision goes out as an engage task at the intercept point. It
  is delivered on the effector's acknowledgement, and `Acknowledged` and `Finished` land on
  the record. The engagement is still open, and PN-06 says the effect was not stated. A
  rejection is recorded with the effector's reason. A plan with no intercept point is never
  sent.
- **`gungnir-ingest`, over real sockets.** Acknowledgement and delivery; refusal of an ack
  from another node or for an unknown task; refusal of a node registered as another type;
  silence and a dead middleware both unreachable; each status's report.
- **The rest.** The task's shape and refusals in `gungnir-sensor-management`, the
  configuration rules, the node's payload choice, and PN-06's sentence.

## What this does not do

- **No binary protobuf bearer.** The SAPIENT detection adapter has the same limit.
- **Task ids live for the session.** A desktop restarted mid-engagement refuses a late
  `TaskAck` as naming a task it did not send, and says so.
- **No effector health line on PN-09.** The link keeps its counters, and an undelivered
  handoff's alert says why. Drawing the counters is a small follow-up.
- **No partner interface.** When a trial partner names its effector's own interface, that
  is a second adapter behind the same `payload_for` and `post`.
- **Human-owned standing.** The delivery path is human-owned. What the owner has signed of
  this change is in `../../signatures.md`.

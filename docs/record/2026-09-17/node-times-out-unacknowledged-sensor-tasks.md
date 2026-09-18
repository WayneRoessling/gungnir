# Node times out unacknowledged sensor tasks

GAP-115 closes. A sensor task the node issued through its SAPIENT router, to a sensor
that never answered, stayed `Sent` on the node indefinitely: its mode request stayed
pending, nothing alerted, and the desktop's handler for a node-published
`SensorTaskEvent::Unacknowledged` (`gungnir-app/src/node_tasks.rs`) had no publisher.

**The desktop already had the rule; the node did not.** The desktop has swept its own
tasks every frame since the outbound control row was built
(`sustainment::sweep_sensor_tasks`). The node issued tasks (`issue_api_tasks`) and
applied acknowledgements (`apply_sapient_task_acks`), but nothing on it ever closed a
window. The GAP-067 walk gated the outbound control row on the crate and the desktop,
and found the gap going looking for the node's half.

**What the node now does, every tick, after applying acknowledgements.**
`sweep_sensor_tasks` asks the registry to time out every task past the baseline's
`sensor_task_ack_window_s`. For each, the registry marks it unacknowledged and withdraws
the pending mode request, so the mode never reads as outstanding for a command nobody
answered. A `tracing::warn!` line serves as the alert, which is how the node raises every
alert, having no operator and no alert list (`record_launch_warnings` does the same).
`SensorTaskEvent::Unacknowledged` goes on the bus, which a connected desktop reads, and
the desktop's existing handler fails its own copy of the task. It runs after the
acknowledgements so a task answered this tick is not also timed out on it.

**It never retries.** DN-11 section 5 rule 3: retry is an operator action, and a node
that quietly re-sent would make the record of what was asked untrue.

**The test uses the real router.** `an_ignored_sapient_task_times_out_once_and_is_never_resent`
builds the node's own `SapientTaskRouter` over a real `SapientTaskAdapter` whose sink
counts each line and never answers. Inside the window nothing happens. Past it the task
is unacknowledged, the mode request is withdrawn, and `Unacknowledged` is published once
with the right task, sensor and time. A later sweep reports nothing new, and the task was
sent exactly once.

# Rs-St Resource states

**UAF definition.** The resource states view shows the states a resource can be in
and the transitions between them.

**Purpose here.** The health and connection states the binaries report, and the
resource-level state machines behind the operational ones in Op-St. Read with the
health panel and the node log.

Status: first draft, 2026-09-04.

## System health (IE-06, reported every tick)

Three booleans, each from the service that owns it: `tracking_healthy` (false until
`PIPELINE_IMPLEMENTED`, and false when the out-of-sequence buffer stalls),
`intercept_healthy` (false when the last solve exceeded its budget or the allocator
returned `NotImplemented`), `ingest_healthy` (false when the number of live adapters
is below the number configured or the gateway is failing). `all_healthy()` is the
conjunction. The UI shows each flag; the node logs the triple on change. There is no
inferred "healthy" state anywhere (CLAUDE.md hard rule).

## Node watchdog (`WatchdogConfig`)

`max_ingest_gap_s` (30 s in the node) and `max_tracking_pipeline_latency_s` (1 s):
a gap beyond the limit warns once and clears when detections resume; pipeline
latency is measured once the pipeline exists.

## Remote backend connection (`gungnir-remote`)

Detached (`connect` returned `TransportNotImplemented` or the endpoint was
unreachable) or Connected (planned, GAP-041). While Detached, `is_healthy()` is
false and the outbox accepts submissions up to `OUTBOX_CAPACITY` (100,000), dropping
and counting the oldest beyond that. The desktop's backend state (Op-St) is derived
from this.

## Journal (`gungnir-store`)

Open (appending), Failed (an append failed; one alert; the session is marked
non-replayable), Closed. Torn tails from a crash are tolerated on read. Retention
purges sessions past the policy age.

## Session (`gungnir-mission::MissionState`)

Live, Replaying, Closed (the lifecycle's save and load are GAP-051).

## Sensor and alert machines

The sensor mode and alert lifecycle machines are resource-level enums drawn in
Op-St; they are not repeated here.

## Elements used

- IE-06, IE-21, IE-27; RS-tracking-service, RS-intercept-service, RS-ingest,
  RS-observability, RS-remote, RS-store, RS-mission.

## Notes

- Every state above is observable from the journal or the log; the
  operational-readiness rows for health (`../../../verification-capability-table.md`
  §2) test the transitions.

## Traceability

- Derives from: `gungnir-model::SystemHealth`, `gungnir-observability`,
  `gungnir-remote`, `gungnir-store`, `gungnir-mission`, the node's watchdog block.
- Feeds: Op-St, Sc-Pr (which state changes are audited), the health verification
  rows.

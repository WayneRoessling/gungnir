# Rs-Pr Resource functions

**UAF definition.** The resource processes view shows the functions a resource
performs and their sequence.

**Purpose here.** The two tick loops that are the product's behaviour today, step by
step from the source, and what each binary does at startup and shutdown. Read by
engineers wiring new crates in and by anyone checking a claim about what the
product does.

Status: first draft, 2026-09-04; the step lists are read from
`gungnir-app/src/update.rs` and `gungnir-node/src/main.rs` and must be updated when
they change.

## RS-app desktop: startup, frame, shutdown

Startup (`gungnir-app/src/main.rs`, `state.rs`): load the config baseline; choose
the backend (`BackendConfig`); on Remote, try `gungnir_remote::connect`, which
returns `TransportNotImplemented`, so fall back to embedded with an alert; construct
`LiveTrackingService`, `DpInterceptService`, the ingest gateway with the allow-list
authenticator and the configured adapters, the bus, the journal, the clock; open a
live session; create the eframe window with `Renderer::Glow`.

Every frame (`update.rs::tick`, budget p99 under 4 ms):

1. `now = clock.now()`.
2. Ingest: `ingest.tick(now, tracking)`; every `IngestEvent` published.
3. Tracking: `tracking.poll(now)`.
4. Planning: `intercept.plan(now, tracks, resources)`; `PlanProposed` published only
   when the plan changed.
5. Health: `SystemHealth` from the three `is_healthy()` calls, never inferred.
6. Journal: every envelope the bus carried this frame appended to the session; a
   failed append raises one alert and marks the session non-replayable.

Then the UI draws the panels of the role's layout and the viewport (2D fallback
until the three-d scene attaches).

Shutdown: the journal is flushed by the file journal's drop; no state is persisted
outside the journal and the config store.

## RS-node service node: startup, tick, shutdown

Startup (`gungnir-node/src/main.rs`): load and validate the baseline from the path
argument or use the default; build the tokio runtime; construct the same services as
the desktop; open the file journal in `data_dir`; open a live session; construct the
gateway with the allow-list authenticator; `ApiServer::serve` logs that no endpoints
are served (transport pending); configure the watchdog.

Every tick (50 ms):

1. `gateway.tick` and publish every ingest event.
2. `tracking.poll`.
3. `intercept.plan`; publish on change.
4. Append every envelope to the journal.
5. Assemble health; log it on change and every 10 s.
6. Watchdog: warn once when no detection has been accepted for longer than the
   ingest-gap limit; clear when detections resume.

Shutdown on Ctrl-C: drain the bus into the journal, log, exit.

## Functions the design adds to both loops

Between planning and health (GAP-028): assessment scores; the policy chain
evaluates the plan; the verdict and rationale accompany it; the approval workflow
opens a pending approval; every decision writes an audit entry (GAP-059). The node
additionally serves the API (GAP-041) and reconciles forwarded journals (GAP-050).

## Elements used

- RS-app, RS-node, RS-tracking-service, RS-intercept-service, RS-ingest,
  RS-eventing, RS-store, RS-time, RS-config, RS-mission, RS-observability, RS-api,
  RS-remote.

## Notes

- Both loops are single-threaded over the services; the tracking pipeline's own
  concurrency lives inside `gungnir-fusion-async` behind `poll`.
- The order "ingest, tracking, planning, events, health, journal" is a standard in
  `../../../rust-ui-architecture-coding-standards.md` §2 and must not change without
  updating this view and Sv-Pr.

## Traceability

- Derives from: `gungnir-app/src/update.rs`, `gungnir-app/src/state.rs`,
  `gungnir-node/src/main.rs`; `../../../performance-budgets.md`.
- Feeds: Sv-Pr, Sv-Cn, Rs-St, the performance harness (GAP-056).

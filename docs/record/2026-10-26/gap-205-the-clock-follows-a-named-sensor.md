# GAP-205: the clock follows a named sensor

Plan 12 tranche A, the third build. The review found that a GNSS-denied site had no clock
discipline at all: `WallClockAuthority` is the host's `SystemTime`, a free-running laptop
against a GPS-disciplined radar drifts past the one-second late-data buffer within hours,
and from then on the tracker drops that radar's reports as late.

## The owner's four choices, 2026-10-26

| Question | Chosen |
|---|---|
| Which sources may discipline? | Any configured sensor, named in `time.discipline`, validated against the baseline's sensors |
| Both binaries, or the desktop? | Both: the node builds the same clock and journals the same events |
| Before the sensor is heard? | The host clock, said: PN-09 reads `waiting`, and the lock is journaled |
| The late-data buffer? | Documented (5 s for an undisciplined host); the one-second default D-98 and D-99 decided stands |

## What was built

- **`gungnir-time::DisciplinedClockAuthority`.** Mission time is the host clock minus the
  least `receipt - source` of the named sensor's reports over a 60 s window: the sample
  with the least transit delay, the reasoning `ClockSkewEstimator` already used. The
  offset applied when a tick's receipts were stamped is passed back with each report, so
  the estimate is formed in host time and the corrected clock never feeds itself. States:
  waiting, locked, holdover after 30 s of silence (the last offset kept), and refused for
  an offset beyond an hour. The first lock and a re-lock after holdover may step the clock;
  otherwise mission time never goes backwards. A test host clock is injectable, and seven
  unit tests cover each state, a host ahead and behind, no feedback over fifty ticks, and
  monotonicity.
- **`gungnir-model`.** `ClockDisciplineState`, `ClockStamps`,
  `HealthEvent::ClockDiscipline`, and an optional `stamps` field on
  `IngestEvent::Quarantined`. All additive and defaulted.
- **The gateway (human-owned).** `validate_detection` is split into `validate_payload`
  and `validate_clock`, unchanged in what it refuses and in what order. A report the clock
  rules alone refused, after authentication and with a sound payload, carries its two
  stamps on the quarantine.
- **Why the gateway had to change.** The gateway refuses a report whose source time is
  more than a second ahead of its receipt. A host behind its radar therefore never
  accepted a radar report. A discipline fed only from accepted reports could never lock in
  exactly the case it exists for. The stamps are taken only after the source has been
  authenticated, so an unauthenticated feed cannot steer the clock.
- **Both binaries.** `gungnir_time::clock_for` builds the clock from the baseline; each
  binary feeds the discipline from accepted and stamped-quarantine reports, journals each
  state change, and the desktop alerts it, resets its skew estimates on a lock, and draws
  the state on PN-09. The strip's clock source reads `sensor-disciplined`. The
  out-of-sync alert now names the two remedies.
- **Configuration.** `time.discipline` with its validation; the late-data documentation;
  a deployment README section, "Time on a site with no NTP or GNSS".

## Tests

`gungnir-app/tests/clock_discipline.rs` is the case end to end: a laptop 40 s behind its
radar has the radar's first report quarantined under the clock rule, hears it through the
stamps, locks, accepts the next report, and measures the radar's skew near zero against
the corrected clock. The node's module test does the same through its bus. The interface
draws every state on PN-09. The configuration refuses a discipline naming a sensor the
baseline does not configure.

## What this does not do

- No own-position: a moving frame origin is a design question before it is a gap, as plan
  12's open questions say.
- The radar's time is trusted as the sensor's own; a sensor whose adapter stamps source
  time from the receiving host would discipline the clock to itself, and PN-09's locked
  offset of zero would say so.
- `gungnir-ingest`'s gateway is human-owned; what the owner has signed of this change is
  in `../../signatures.md`.
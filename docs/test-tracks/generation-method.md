# Generation method

Status: first draft, 2026-09-04. How a test-track set is produced, why it is
reproducible, how classes and scenarios are configured, and how the output is
validated. The reference generator is `tools/gen_tracks.py`; the `gungnir-scenario`
generator (GAP-016) must reproduce its output for the same inputs and seed.

## 1. Inputs

| Input | Owns | Consumed by |
|---|---|---|
| `catalogue.yaml` and `catalogue-{air,sea,land}.yaml` | platform envelopes, signature classes, emissions, sources, confidence | catalogue rendering; the generator for per-platform signature and emission |
| `classes.yaml` | class envelope, phases with movement models, randomization rules | class-profile rendering; the generator's movement |
| `sensors.yaml` | sensor types: range bands, noise, timing, dropouts, false alarms, electronic-attack sensitivities | sensor-model rendering; the generator's observation |
| `scenarios.yaml` | geography, routes, sensor sets, scenario composition, events, expected outcomes, sample reduction | scenario-library rendering; the generator's composition |

The four files are the single source; the Markdown under this folder is rendered
from them by `tools/build_catalogue.py` and must not be edited by hand.

## 2. Determinism and seeding

- One `random.Random(seed)` per set; the seed is the scenario's `sample.seed` for
  the sample variant and `seed + 1000` for the full variant; every draw (spawn
  times, phase parameters, route jitter, noise, dropouts, false alarms, latency)
  comes from it in a fixed order.
- The same generator version, inputs, and seed produce byte-identical files;
  `metadata.json` records all of them, so a set can be regenerated instead of
  stored.
- Floating-point results are rounded on output (positions to 0.1 m in truth, 0.01 m
  in detections, times to 1 ms) to keep the files stable across platforms.

## 3. Composition

1. **Resolve the scenario.** `base: TT-01` and `entities: inherit` copy another
   scenario's entities; `variants` move sensors; the sample block reduces duration,
   scales entity counts (at least one per group), narrows sensor sets, and may
   replace events.
2. **Instantiate sensors** from the named sensor sets, applying instance overrides
   (for example the injected bias of ISR sortie 1 in TT-06), with a random phase
   offset per sensor so scans are not aligned.
3. **Instantiate entities** per group: a spawn time in the group's window (clamped
   to the first 70 percent of the duration), the route with per-entity lateral
   jitter (2 km air, 300 m sea, 20 m land) and optional group spacing, the class's
   phases (or the subset the group names) with parameters drawn from the phase
   ranges, and flags (decoy, no terminal, emitting, cooperative identity, AIS off or
   spoofed, occlusion window).
4. **Interceptors** are entities with `launch` and `target_group`; each is assigned
   a target in that group and pursues its current position.

## 4. Movement

Each truth tick (1 s full, 2 s sample) every spawned, alive entity advances by its
phase's model (`classes.yaml` header): waypoint following with heading slew limited
by the class's lateral acceleration, altitude bands with nap-of-the-earth jitter,
orbits, hovers, road moves with random halts, shoot-and-move with a `fires` event,
dash-terminal pursuit ending at the target, glide, and ballistic arcs. Classes that
end on a target switch to their terminal phase within 3 km of the last waypoint;
`no_terminal` decoys fly past. Speed stays inside the class envelope by
construction, which the validator confirms.

## 5. Observation

Per the numbered procedure in `sensor-models.md`: opportunities per sensor period,
range band by signature class, cooperative and cued rules, horizon, field of
regard, altitude limits, probability of detection with dropouts and
electronic-attack multipliers, noise in the line-of-sight frame plus bias and AIS
spoof offsets, source time with a lagging-clock skew, receipt time with latency,
jitter, and out-of-order delay capped below the gateway's 5 s window, and Poisson
false alarms. Detections are written in receipt order; `detections-truth.jsonl`
maps each line to the entity that caused it (or none for a false alarm) so that
association and identification can be scored.

## 6. Output and validation

The set layout is `data-format.md`. `tools/validate_tracks.py` runs the checks in
`validation.md`, writes `validation-report.json`, and records the outcome in
`metadata.json`; a set is not committed unless it passed.

## 7. Configuring a new class or scenario

- New class: add it to `classes.yaml` with an envelope that contains every platform
  you then add to a `catalogue-*.yaml` file with sources and confidence; the check
  fails until both exist. Add its sensors and threads.
- New scenario: add points and routes if needed, then a scenario entry with entity
  groups, sensor sets, events, expected outcomes, and a `sample` block; give it a
  vignette and thread.
- Run `build_catalogue.py`, `gen_tracks.py <id>`, `validate_tracks.py`; commit the
  YAML, the rendered Markdown, and the sample set together.

## 8. Full-size generation

`gen_tracks.py --full TT-01` writes into `testdata/tracks/full/` (ignored by version
control) with the full duration, counts, sensor sets, and a 1 s truth tick. TT-01
full is about 45 entities over 70 minutes with 12 sensors, on the order of a few
hundred thousand detections; generation takes minutes in the reference generator
and is the size the `gungnir-scenario` generator and the benchmarks are for.

## 9. What the reference generator does not do

- No terrain: coverage uses the radar-horizon rule and altitude limits, not line
  of sight over the Hoge ridge; the `gungnir-analytics` line-of-sight will replace
  this in the Rust generator.
- No sensor tasking loop: cued cameras follow any prior detection rather than a
  sensor manager's cue.
- No effector outcomes except interceptor pursuit; engagement assessment truth
  (GAP-043) is a later addition.
- Electronic attack is its effect on sensors only (`sourcing-and-legal.md` §4).

## Traceability

- `../scenario-crate-narrative.md` (the five engineering scenarios the library
  extends), `gungnir-scenario` (GAP-016), `../verification-capability-table.md` §1
  and §2, MOP-25 (per-class prediction tolerances come from these truths).

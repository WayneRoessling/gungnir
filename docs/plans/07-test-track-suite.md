# Plan 07: Test-track suite for vehicles in the Russia-Ukraine war

## Purpose

Build a catalogue and generator for realistic test tracks (ground-truth trajectories
and simulated sensor observations) covering the air, sea, and land platforms in use
by both belligerents in the Russia-Ukraine war, so that the tracking core, the
service facades, the intercept planner, the UI, and the ML models are exercised
against the movement profiles a fielded system will meet. The suite feeds the
verification table, the end-to-end scenario replay, demonstrations, and ML training.

## Scope

In scope: a vehicle catalogue with kinematic envelopes and coarse signature classes
from open sources; per-class track profile specifications; sensor models that turn
truth into observations; a scenario library aligned with the mission vignettes; the
data format; the generator (extending `gungnir-scenario`); validation; and the
sourcing policy.

Out of scope: detailed radar, infrared, or acoustic signature models beyond a coarse
class; anything derived from controlled, classified, or proprietary data; and
adversary tactics beyond what public reporting describes.

## Inputs

- `docs/mission/vignettes.md` and `operational-environment.md` (plan 02).
- `docs/scenario-crate-narrative.md` and `gungnir-scenario` for the existing
  generator and the five engineering scenarios.
- `gungnir-model::DetectionView`, `gungnir-ingest::adapters::recorded` (the
  JSON-lines feed format), `gungnir-interop` (the Arrow form).
- Open sources for platform performance: manufacturer and public specifications,
  open-source intelligence trackers of equipment in use, published analyses. Each
  figure is recorded with its source and a confidence mark.

## Vehicle catalogue (initial classes)

The catalogue is organized by kinematic class, because the tracker cares about how
things move; specific platforms are examples within a class with their own envelope
values.

| Domain | Class | Example platforms (both sides) |
|---|---|---|
| Air | One-way attack UAS, propeller | Shahed-136 and Geran-2 family, Ukrainian long-range strike UAS |
| Air | One-way attack UAS, jet | Jet-powered Geran variants |
| Air | Loitering munition | Lancet, Switchblade class |
| Air | Small multirotor and FPV | DJI Mavic class, FPV strike quadcopters |
| Air | Tactical fixed-wing ISR UAS | Orlan-10, Furia, Leleka class |
| Air | Medium-altitude long-endurance UAS | Bayraktar TB2, Orion class |
| Air | Cruise missile, subsonic | Kalibr, Kh-101, Storm Shadow and SCALP, Neptune |
| Air | Cruise missile, supersonic and aeroballistic | Kh-22 and Kh-32, Kinzhal, Zircon class |
| Air | Ballistic missile, short range | Iskander-M, ATACMS, KN-23 class |
| Air | Glide bomb | KAB with UMPK kits, JDAM-ER class |
| Air | Tactical fixed-wing aircraft | Su-25, Su-34, Su-35, MiG-29, F-16 |
| Air | Rotary wing | Ka-52, Mi-8, Mi-24 and Mi-35 |
| Air | Air-defense interceptor missile | Patriot, NASAMS, S-300 and S-400 interceptors (for deconfliction and engagement assessment) |
| Sea | Uncrewed surface vessel | Magura V5, Sea Baby class |
| Sea | Fast craft and patrol boat | Raptor, Gyurza-M class |
| Sea | Surface combatant | Corvette and frigate classes of the Black Sea Fleet |
| Sea | Amphibious and auxiliary | Landing ships, tankers |
| Sea | Submarine (surfaced or snorkelling only) | Kilo class, for surface-picture purposes only |
| Land | Main battle tank | T-72, T-80, T-90, Leopard 2, Challenger 2, M1 Abrams |
| Land | Infantry fighting vehicle | BMP-2 and BMP-3, Bradley, CV90, Marder |
| Land | Armoured personnel carrier and MRAP | BTR-82, M113, Stryker, MaxxPro |
| Land | Self-propelled artillery | 2S19 Msta-S, 2S3, PzH 2000, Caesar, M109, Krab |
| Land | Rocket artillery | BM-21 Grad, Tornado-S, HIMARS, M270 |
| Land | Mobile air-defense system | Buk, Tor, Pantsir, Gepard, Patriot and NASAMS launcher vehicles |
| Land | Electronic-warfare vehicle | Krasukha, Leer, Bukovel class |
| Land | Logistics truck | KamAZ, Ural, HEMTT class |
| Land | Uncrewed ground vehicle | Small logistics and engineering UGVs |

Every platform entry carries: class, side or operator, role, speed range, altitude or
depth range where applicable, climb and dive rates, turn-rate or lateral acceleration
limits, endurance, typical mission profile phases, coarse radar cross-section class,
coarse infrared class, acoustic class, emission behaviour where public, sources, and
a confidence mark.

## Deliverables and target location

Documents under `docs/test-tracks/`; data under `testdata/tracks/`.

| Path | Content |
|---|---|
| `docs/test-tracks/README.md` | Index, how to generate, how to use in tests |
| `docs/test-tracks/vehicle-catalogue.md` | The catalogue above, one table per domain, plus per-platform detail pages under `platforms/` |
| `docs/test-tracks/class-profiles/` | One file per kinematic class: phases of a representative mission, parameter ranges, manoeuvre models (constant velocity, coordinated turn, terminal dive, weaving, loiter), randomization rules |
| `docs/test-tracks/sensor-models.md` | Radar, electro-optical and infrared, acoustic, radio-frequency detection, and cooperative-source models: detection probability versus range and signature class, measurement noise, update rate, field of regard, clutter, dropouts, latency and out-of-order behaviour, electronic-attack degradation |
| `docs/test-tracks/scenario-library.md` | Scenarios composed from class profiles and sensor models, one or more per mission vignette: raid compositions, timings, geography, defended assets, expected outcomes |
| `docs/test-tracks/data-format.md` | Truth and observation file formats (JSON-lines `DetectionView`, truth records, Arrow), naming, metadata, versioning, provenance fields |
| `docs/test-tracks/generation-method.md` | How the generator works, determinism and seeding, how classes and scenarios are configured, how outputs are validated |
| `docs/test-tracks/validation.md` | Checks each generated set must pass: envelope limits, physical plausibility, statistical self-checks from the verification table, subject-matter review |
| `docs/test-tracks/sourcing-and-legal.md` | Open-source-only policy, source recording, confidence marks, exclusion of controlled data, export-control note |
| `testdata/tracks/` | Small committed sample sets per scenario; large sets generated on demand and not committed |

## Engineering work the plan schedules

- Extend `gungnir-scenario` with class-profile generators and scenario composition
  (new `Scenario` variants or a `ScenarioSpec` loaded from the library), keeping every
  stochastic element behind an explicit `Rng` per the coding standards.
- A `gungnir-scenario` writer for the data format and a reader in
  `gungnir-ingest::adapters::recorded` (already reads JSON-lines detections).
- Sensor models as a module of `gungnir-scenario` with the parameters above.
- Validation as tests: envelope and plausibility checks per class; the statistical
  self-check rows from `docs/verification-capability-table.md` §1.
- A `gungnir-fuzz` corpus seeded from the sample sets.
- Wiring the sample sets into the end-to-end scenario replay (`ARCHITECTURE.md` §6)
  and into `bench-regression.yml` inputs.

## Method

1. **Sourcing policy first.** Write `sourcing-and-legal.md`; it governs everything
   after it.
2. **Catalogue.** Populate the catalogue from open sources with per-figure sources
   and confidence marks; subject-matter review of each domain table.
3. **Class profiles.** Write one profile per class with parameter ranges and
   manoeuvre models; review for plausibility.
4. **Sensor models.** Specify per sensor type; align with `gungnir-time` late-data
   semantics and the out-of-sequence behaviour the tracking core must handle.
5. **Scenario library.** Compose scenarios from the vignettes; each names the
   classes, counts, timing, sensors, and expected outcomes.
6. **Generator.** Implement in `gungnir-scenario`; deterministic under a seed;
   validated by the checks in `validation.md`.
7. **Generate and validate** the sample sets; commit small sets; document how to
   regenerate large ones.
8. **Integrate** into tests, replay, demonstrations, and the ML data pipeline.

## Roles

- Owner: sourcing policy sign-off, domain priorities.
- Data agent: catalogue population with sources, class profiles, scenario library.
- Engineering agent: generator, sensor models, validation tests, integration.
- Subject-matter reviewers (human-owned): vehicle data vetting, plausibility of
  profiles and scenarios.

## Dependencies

Plan 02 for vignettes. The generator work depends on the five-scenario generator
being implemented in `gungnir-scenario` (`ARCHITECTURE.md` §10); the catalogue and
profiles do not.

## Effort and sequencing

20 to 30 agent-assisted days; 5 weeks elapsed, starting after plan 02. The catalogue
and profiles can start immediately.

## Acceptance criteria

- Every class has a profile and at least one platform entry with sourced figures.
- Every mission vignette has at least one scenario in the library.
- Every generated set passes the validation checks and carries provenance metadata.
- The sample sets replay through the ingest gateway and the service facades without
  quarantine, and they are used by at least one CI test and the benchmark inputs.
- The sourcing policy is followed and auditable: every figure has a source.

## Risks

- Open-source figures are inconsistent or wrong; mitigate with ranges, confidence
  marks, and plausibility checks rather than single values.
- Scope creep across hundreds of platforms; mitigate by working class by class and
  prioritizing the lead mission's classes.
- Export-control sensitivity of compiled performance data; mitigate by keeping to
  published figures, recording sources, and asking counsel (plan 01) whether the
  compiled catalogue needs a review before external release.

## Open questions

- Which classes are first: proposed order is one-way attack UAS, cruise missiles,
  small UAS, uncrewed surface vessels, then land classes.
- Whether the catalogue is released externally with the product or kept internal.
- The size limit for committed sample sets.

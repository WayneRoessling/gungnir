# One-way attack UAS, jet (`air.owa-jet`)

Turbojet variant of the one-way attack drone; faster ingress, same route discipline, shorter endurance.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 70 to 185 | 0 to 5000 | -120 to 16 | 0.3 to 1.6 | 0 to 4 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| ingress | `waypoint-cruise` | 600 to 3600 | 80 to 150 | 300 to 2000 | weave=0.02 |
| valley-run | `nap-of-earth` | 120 to 900 | 100 to 170 | 60 to 300 | weave=0.04 |
| terminal | `dash-terminal` | 10 to 40 | 120 to 180 | 0 to 300 |  |

## Randomization

- speed: per-entity
- altitude: per-phase
- route: 2 km jitter

## Signatures and sensors

Radar cross-section small, infrared medium, acoustic loud. Seen by: `radar.long`, `radar.medium`, `radar.short`, `acoustic`, `eo-ir` (`../sensor-models.md`).

## Platforms in this class

- [Jet-powered Geran variant (Geran-3 class)](../platforms/geran-3-jet.md) (red, confidence low)

## Threads and scenarios

Threads MT-01, MT-02; scenarios TT-02 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

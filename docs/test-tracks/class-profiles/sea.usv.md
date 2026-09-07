# Uncrewed surface vessel (`sea.usv`)

Small, fast, low craft in a loose group, no AIS, straight runs with occasional jinks; lost and reacquired in clutter.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 3 to 26 | 0 | 0 | 0.1 to 0.9 | 0 to 2 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `sea-transit` | 1200 to 7200 | 10 to 20 | 0 |  |
| attack-run | `sea-transit` | 200 to 900 | 18 to 25 | 0 | weave=0.05 |
| terminal | `dash-terminal` | 20 to 120 | 20 to 26 | 0 |  |

## Randomization

- group spacing: per-scenario 300 to 1500 m
- jinks: per-entity
- target ship: per-entity

## Signatures and sensors

Radar cross-section very-small, infrared low, acoustic moderate. Seen by: `radar.coastal`, `eo-ir`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [Magura V5](../platforms/magura-v5.md) (blue, confidence medium)
- [Sea Baby class](../platforms/sea-baby.md) (blue, confidence low)

## Threads and scenarios

Threads MT-04, MT-05; scenarios TT-04 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

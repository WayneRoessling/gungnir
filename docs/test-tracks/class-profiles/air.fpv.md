# FPV strike quadcopter (`air.fpv`)

Fast, low, short-range strike quadcopter flown by video; approaches from cover at tree-top height and dives on the target; fibre-optic variants emit nothing.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 10 to 50 | 0 to 300 | -40 to 16 | 0.5 to 4.5 | 0 to 10 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| approach | `nap-of-earth` | 30 to 180 | 20 to 40 | 3 to 40 | weave=0.1 |
| terminal | `dash-terminal` | 5 to 20 | 30 to 45 | 0 to 40 |  |

## Randomization

- launch point: per-entity from a tree line 1 to 3 km out
- emitting: per-entity 70 percent radio and 30 percent fibre-optic

## Signatures and sensors

Radar cross-section very-small, infrared low, acoustic quiet. Seen by: `radar.short`, `rf`, `eo-ir`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [FPV strike quadcopter (7 to 10 inch)](../platforms/fpv-strike-quad.md) (both, confidence medium)

## Threads and scenarios

Threads MT-03; scenarios TT-03 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

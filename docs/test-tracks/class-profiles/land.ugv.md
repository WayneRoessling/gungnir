# Uncrewed ground vehicle (`land.ugv`)

Small, slow ground robot on a supply or engineering task; small return; control link.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 7 | 0 | 0 | 0.05 to 0.7 | 0 to 1 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| task | `road-move` | 600 to 7200 | 1 to 5 | 0 | stop_prob=0.1 |

## Randomization

- route: per-entity

## Signatures and sensors

Radar cross-section small, infrared low, acoustic quiet. Seen by: `radar.ground`, `isr-video`, `rf` (`../sensor-models.md`).

## Platforms in this class

- [Small logistics and engineering UGVs](../platforms/small-ugv.md) (both, confidence low)

## Threads and scenarios

Threads MT-06; scenarios TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

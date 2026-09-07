# Main battle tank (`land.mbt`)

Tracked heavy vehicle in column or dispersed movement, stops under cover; loud and hot; seen by ground radar and ISR video.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 21 | 0 | 0 | 0.05 to 0.6 | 0 to 1.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| road-move | `road-move` | 600 to 7200 | 8 to 18 | 0 | stop_prob=0.05 |
| cross-country | `road-move` | 300 to 3600 | 3 to 11 | 0 | stop_prob=0.1 |
| cover | `stationary` | 300 to 3600 | 0 | 0 |  |

## Randomization

- column spacing: per-scenario 50 to 150 m
- stops: per-tick
- cover gaps: per-entity

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.ground`, `isr-video`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [T-72 / T-80 / T-90 family](../platforms/t-72.md) (both, confidence high)
- [Leopard 2 / Challenger 2 / M1 Abrams](../platforms/leopard-2.md) (blue, confidence high)

## Threads and scenarios

Threads MT-06; scenarios TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

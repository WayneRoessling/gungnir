# Infantry fighting vehicle (`land.ifv`)

Tracked medium vehicle moving with tanks or infantry; similar kinematics to the tank, slightly faster.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 21 | 0 | 0 | 0.05 to 0.6 | 0 to 1.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| road-move | `road-move` | 600 to 7200 | 8 to 19 | 0 | stop_prob=0.05 |
| cross-country | `road-move` | 300 to 3600 | 3 to 12 | 0 | stop_prob=0.1 |
| cover | `stationary` | 300 to 3600 | 0 | 0 |  |

## Randomization

- as land.mbt: None

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.ground`, `isr-video`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [BMP-2 / BMP-3](../platforms/bmp-2.md) (both, confidence high)
- [M2 Bradley / CV90 / Marder](../platforms/bradley.md) (blue, confidence high)

## Threads and scenarios

Threads MT-06; scenarios TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

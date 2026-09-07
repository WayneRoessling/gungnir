# Armoured personnel carrier and MRAP (`land.apc`)

Wheeled armoured vehicle, faster on roads, in convoys with trucks.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 30 | 0 | 0 | 0.05 to 0.6 | 0 to 2 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| road-move | `road-move` | 600 to 7200 | 10 to 25 | 0 | stop_prob=0.04 |
| cover | `stationary` | 300 to 3600 | 0 | 0 |  |

## Randomization

- as land.mbt: None

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.ground`, `isr-video`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [BTR-82 / BTR-80](../platforms/btr-82.md) (both, confidence high)
- [Stryker / M113 / MaxxPro](../platforms/stryker.md) (blue, confidence high)

## Threads and scenarios

Threads MT-06; scenarios TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

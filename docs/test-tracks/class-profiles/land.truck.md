# Logistics truck (`land.truck`)

Trucks in convoy on roads at night, regular spacing, stops at halts, under tree cover in gaps.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 30 | 0 | 0 | 0.05 to 0.5 | 0 to 1.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| convoy | `road-move` | 1200 to 14400 | 8 to 20 | 0 | stop_prob=0.03 |
| halt | `stationary` | 300 to 1800 | 0 | 0 |  |

## Randomization

- convoy spacing: per-scenario 50 to 100 m
- halts: per-scenario
- cover gaps: per-scenario

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.ground`, `isr-video`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [KamAZ / Ural / HEMTT logistics trucks](../platforms/kamaz-truck.md) (both, confidence high)

## Threads and scenarios

Threads MT-06; scenarios TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

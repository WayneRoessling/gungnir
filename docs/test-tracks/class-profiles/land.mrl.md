# Rocket artillery (`land.mrl`)

Multiple rocket launcher on a truck or tracked chassis; shoot-and-move with shorter dwell than tube artillery; the launch is a bright, loud event.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 27 | 0 | 0 | 0.05 to 0.5 | 0 to 1.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| move-in | `road-move` | 300 to 1800 | 8 to 20 | 0 | stop_prob=0.03 |
| fire | `shoot-and-move` | 60 to 240 | 0 | 0 | fire_stop_s=45 to 150 |
| displace | `road-move` | 300 to 1200 | 10 to 22 | 0 | stop_prob=0.02 |

## Randomization

- as land.spg: None

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.ground`, `isr-video`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [BM-21 Grad / Tornado-S](../platforms/bm-21-grad.md) (both, confidence high)
- [M142 HIMARS / M270](../platforms/himars.md) (blue, confidence high)

## Threads and scenarios

Threads MT-06; scenarios TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

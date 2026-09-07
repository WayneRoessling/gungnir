# Self-propelled artillery (`land.spg`)

Battery of self-propelled howitzers that moves into a firing position, fires, and moves within minutes; the firing signature is the cue.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 29 | 0 | 0 | 0.05 to 0.5 | 0 to 1.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| move-in | `road-move` | 300 to 1800 | 6 to 16 | 0 | stop_prob=0.03 |
| fire | `shoot-and-move` | 120 to 480 | 0 | 0 | fire_stop_s=90 to 300 |
| displace | `road-move` | 300 to 1200 | 8 to 18 | 0 | stop_prob=0.02 |
| hide | `stationary` | 600 to 7200 | 0 | 0 |  |

## Randomization

- firing position: per-entity from the scenario wood lines
- dwell: per-entity
- battery spacing: 100 to 300 m

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.ground`, `isr-video`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [2S19 Msta-S / 2S3 Akatsiya](../platforms/2s19-msta-s.md) (both, confidence high)
- [PzH 2000 / Caesar / M109 / Krab](../platforms/pzh-2000.md) (blue, confidence high)

## Threads and scenarios

Threads MT-06; scenarios TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

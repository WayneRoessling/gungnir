# Electronic-warfare vehicle (`land.ew`)

Vehicle that moves to a site and emits jamming; the model's effect is on sensors (clock skew, dropouts), the vehicle itself is a truck-like track.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 26 | 0 | 0 | 0.05 to 0.5 | 0 to 1.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| move | `road-move` | 300 to 3600 | 6 to 18 | 0 | stop_prob=0.03 |
| operate | `stationary` | 1800 to 36000 | 0 | 0 | emitting=True |

## Randomization

- position: from the scenario

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.ground`, `isr-video`, `rf` (`../sensor-models.md`).

## Platforms in this class

- [Krasukha / Leer-3 electronic-warfare vehicles](../platforms/krasukha.md) (red, confidence medium)
- [Bukovel-AD class counter-UAS EW](../platforms/bukovel-ad.md) (blue, confidence low)

## Threads and scenarios

Threads MT-07; scenarios TT-07 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

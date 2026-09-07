# Mobile air-defense system (`land.mobile-ad`)

Radar and launcher vehicles that move between positions and emit when operating; own systems appear as blue-force tracks.

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

- position: from the scenario laydown

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.ground`, `isr-video`, `rf`, `bft` (`../sensor-models.md`).

## Platforms in this class

- [Buk / Tor / Pantsir vehicles](../platforms/buk.md) (red, confidence high)
- [Patriot and NASAMS launcher vehicles; Gepard](../platforms/patriot-launcher.md) (blue, confidence high)

## Threads and scenarios

Threads MT-06, MT-09; scenarios TT-06, TT-09 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

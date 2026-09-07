# Small multirotor (`air.small-multirotor`)

Consumer-class quadcopter for reconnaissance or drops; hovers, transits slowly, returns toward its operator; emits a control link.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 25 | 0 to 600 | -10 to 10 | 0.3 to 2.5 | 0 to 6 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| approach | `waypoint-cruise` | 60 to 600 | 8 to 18 | 30 to 150 |  |
| observe | `hover` | 60 to 900 | 0 to 2 | 40 to 150 |  |
| return | `waypoint-cruise` | 60 to 600 | 10 to 20 | 30 to 150 |  |

## Randomization

- operator position: per-entity within 5 km of the site
- hover point: per-entity over the site

## Signatures and sensors

Radar cross-section very-small, infrared low, acoustic quiet. Seen by: `radar.short`, `rf`, `eo-ir`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [DJI Mavic 3 class multirotor](../platforms/mavic-3.md) (both, confidence high)

## Threads and scenarios

Threads MT-03; scenarios TT-03 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

# Air-defense interceptor missile (`air.interceptor`)

Own interceptor from launch to intercept; modelled so the tracker and deconfliction see it; very fast, short-lived.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 500 to 2100 | 0 to 30000 | -1500 to 1500 | 2.0 to 30.0 | 0 to 300 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| fly-out | `dash-terminal` | 15 to 120 | 900 to 1800 | 0 to 25000 |  |

## Randomization

- launch site: from the scenario resources
- target: the assigned track

## Signatures and sensors

Radar cross-section small, infrared high, acoustic loud. Seen by: `radar.long`, `radar.medium` (`../sensor-models.md`).

## Platforms in this class

- [Patriot PAC-2 and PAC-3 interceptors](../platforms/pac-3.md) (blue, confidence medium)
- [NASAMS (AIM-120 class) interceptor](../platforms/nasams-amraam.md) (blue, confidence medium)
- [S-300 and S-400 family interceptors (48N6 class)](../platforms/s-400-interceptor.md) (red, confidence low)

## Threads and scenarios

Threads MT-02, MT-06; scenarios TT-02, TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

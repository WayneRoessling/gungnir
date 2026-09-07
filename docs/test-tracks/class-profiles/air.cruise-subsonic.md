# Cruise missile, subsonic (`air.cruise-subsonic`)

Terrain-following subsonic missile on a pre-planned route with waypoints, low over water and valleys, popping up or diving at the target; minutes of warning at most.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 180 to 320 | 0 to 6000 | -200 to 60 | 0.5 to 4.5 | 0 to 10 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| cruise | `nap-of-earth` | 300 to 7200 | 230 to 300 | 30 to 150 | weave=0.01 |
| valley-run | `nap-of-earth` | 60 to 600 | 240 to 300 | 30 to 100 | weave=0.02 |
| terminal | `dash-terminal` | 10 to 40 | 250 to 310 | 0 to 1000 |  |

## Randomization

- route: waypoints jittered 1 km
- altitude: per-phase
- decoy missile: same profile with rcs_class medium and no terminal phase (flies past)

## Signatures and sensors

Radar cross-section small, infrared medium, acoustic loud. Seen by: `radar.long`, `radar.medium`, `radar.short`, `eo-ir`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [Kalibr (3M-14) land-attack cruise missile](../platforms/kalibr.md) (red, confidence medium)
- [Kh-101](../platforms/kh-101.md) (red, confidence medium)
- [Storm Shadow / SCALP-EG](../platforms/storm-shadow.md) (blue, confidence high)
- [R-360 Neptune](../platforms/neptune.md) (blue, confidence medium)

## Threads and scenarios

Threads MT-02; scenarios TT-02 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

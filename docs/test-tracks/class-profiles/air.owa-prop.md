# One-way attack UAS, propeller (`air.owa-prop`)

Long-endurance propeller drone flown on a pre-planned route at low altitude to a fixed target; streams of tens; decoys mixed in; terminal dive on the target.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 25 to 62 | 0 to 4000 | -60 to 6 | 0.2 to 1.2 | 0 to 2 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| ingress | `waypoint-cruise` | 600 to 7200 | 40 to 52 | 300 to 1500 | weave=0.02 |
| valley-run | `nap-of-earth` | 300 to 1800 | 42 to 52 | 60 to 300 | weave=0.05 |
| terminal | `dash-terminal` | 20 to 90 | 45 to 60 | 0 to 300 |  |

## Randomization

- speed: per-entity uniform in the phase range
- altitude: per-phase uniform in the band
- route: waypoints jittered up to 2 km laterally per entity
- spawn: per scenario stream
- decoy: a decoy uses the same profile with rcs_class large

## Signatures and sensors

Radar cross-section small, infrared low, acoustic loud. Seen by: `radar.long`, `radar.medium`, `radar.short`, `acoustic`, `eo-ir` (`../sensor-models.md`).

## Platforms in this class

- [Shahed-136 / Geran-2 family](../platforms/shahed-136.md) (red, confidence medium)
- [Ukrainian long-range strike UAS (Liutyi, UJ-22 class)](../platforms/ua-long-range-strike-uas.md) (blue, confidence low)

## Threads and scenarios

Threads MT-01, MT-02, MT-07, MT-09, MT-10; scenarios TT-01, TT-02, TT-07, TT-09, TT-10 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

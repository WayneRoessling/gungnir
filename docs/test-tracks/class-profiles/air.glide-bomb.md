# Glide bomb (`air.glide-bomb`)

Released from an aircraft at altitude tens of kilometres out and glides to the target; steady descent, no propulsion, small return.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 150 to 320 | 0 to 15000 | -100 to 0 | 0.3 to 2.5 | 0 to 5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| glide | `glide` | 90 to 300 | 180 to 300 | 0 to 12000 | glide_ratio=5 to 9 |

## Randomization

- release altitude and range: per-entity
- aimpoint: per-entity from the target list

## Signatures and sensors

Radar cross-section small, infrared low, acoustic quiet. Seen by: `radar.long`, `radar.medium` (`../sensor-models.md`).

## Platforms in this class

- [KAB with UMPK glide kit (FAB-500 to FAB-1500 class)](../platforms/kab-umpk.md) (red, confidence medium)
- [JDAM-ER class glide bomb](../platforms/jdam-er.md) (blue, confidence medium)

## Threads and scenarios

Threads MT-02; scenarios TT-02 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

# Cruise missile, supersonic and aeroballistic (`air.cruise-supersonic`)

High-altitude supersonic cruise or aeroballistic lob followed by a steep, fast terminal dive; seconds of decision time; large radar return at altitude.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 800 to 2100 | 0 to 45000 | -1500 to 600 | 0.5 to 10.0 | 0 to 50 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| high-cruise | `waypoint-cruise` | 120 to 900 | 900 to 1500 | 12000 to 30000 |  |
| terminal | `dash-terminal` | 20 to 60 | 1000 to 2000 | 0 to 30000 |  |

## Randomization

- cruise altitude: per-entity
- terminal dive angle: per-entity 30 to 70 degrees

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.long`, `radar.medium` (`../sensor-models.md`).

## Platforms in this class

- [Kh-22 / Kh-32](../platforms/kh-22.md) (red, confidence low)
- [Kh-47M2 Kinzhal](../platforms/kinzhal.md) (red, confidence low)

## Threads and scenarios

Threads MT-02; scenarios TT-02 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

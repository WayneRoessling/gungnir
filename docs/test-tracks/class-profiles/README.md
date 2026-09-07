# Class profiles

Status: rendered by `tools/build_catalogue.py` from `../classes.yaml` (version 2026-09-04) on 2026-09-04. One file per kinematic class: the envelope every platform in the class fits, the phases of a representative mission with the movement model per phase, the randomization rules, signatures, the sensors that see the class, and the threads and scenarios that use it. The movement models are implemented in `../tools/gen_tracks.py` and are the specification for the `gungnir-scenario` generator (GAP-016).

| Class | Name | Platforms | Phases | Scenarios |
|---|---|---|---|---|
| [`air.owa-prop`](air.owa-prop.md) | One-way attack UAS, propeller | 2 | ingress, valley-run, terminal | TT-01, TT-02, TT-07, TT-09, TT-10 |
| [`air.owa-jet`](air.owa-jet.md) | One-way attack UAS, jet | 1 | ingress, valley-run, terminal | TT-02 |
| [`air.loitering`](air.loitering.md) | Loitering munition | 2 | transit, loiter, terminal | TT-03, TT-06 |
| [`air.small-multirotor`](air.small-multirotor.md) | Small multirotor | 1 | approach, observe, return | TT-03 |
| [`air.fpv`](air.fpv.md) | FPV strike quadcopter | 1 | approach, terminal | TT-03 |
| [`air.tactical-isr`](air.tactical-isr.md) | Tactical fixed-wing ISR UAS | 2 | transit, orbit, egress | TT-06, TT-08 |
| [`air.male-uas`](air.male-uas.md) | Medium-altitude long-endurance UAS | 2 | transit, orbit | TT-08 |
| [`air.cruise-subsonic`](air.cruise-subsonic.md) | Cruise missile, subsonic | 4 | cruise, valley-run, terminal | TT-02 |
| [`air.cruise-supersonic`](air.cruise-supersonic.md) | Cruise missile, supersonic and aeroballistic | 2 | high-cruise, terminal | TT-02 |
| [`air.ballistic-srbm`](air.ballistic-srbm.md) | Ballistic missile, short range | 3 | arc | TT-02 |
| [`air.glide-bomb`](air.glide-bomb.md) | Glide bomb | 2 | glide | TT-02 |
| [`air.tactical-fixed-wing`](air.tactical-fixed-wing.md) | Tactical fixed-wing aircraft | 3 | transit, run-in, evade, egress | TT-02, TT-08 |
| [`air.rotary-wing`](air.rotary-wing.md) | Rotary wing | 2 | transit, hover, egress | TT-04, TT-08 |
| [`air.civil-airliner`](air.civil-airliner.md) | Civil airliner | 1 | transit | TT-08 |
| [`air.light-aircraft`](air.light-aircraft.md) | Light aircraft | 1 | transit | TT-08 |
| [`air.interceptor`](air.interceptor.md) | Air-defense interceptor missile | 3 | fly-out | TT-02, TT-06 |
| [`sea.usv`](sea.usv.md) | Uncrewed surface vessel | 2 | transit, attack-run, terminal | TT-04 |
| [`sea.fast-craft`](sea.fast-craft.md) | Fast craft and patrol boat | 2 | patrol, intercept | TT-04, TT-05 |
| [`sea.surface-combatant`](sea.surface-combatant.md) | Surface combatant | 2 | transit, station | TT-05 |
| [`sea.amphibious-auxiliary`](sea.amphibious-auxiliary.md) | Amphibious, auxiliary, and civil traffic | 3 | transit, anchored, loiter | TT-04, TT-05 |
| [`sea.submarine-surfaced`](sea.submarine-surfaced.md) | Submarine, surfaced or snorkelling | 1 | surfaced-transit | TT-05 |
| [`land.mbt`](land.mbt.md) | Main battle tank | 2 | road-move, cross-country, cover | TT-06 |
| [`land.ifv`](land.ifv.md) | Infantry fighting vehicle | 2 | road-move, cross-country, cover | TT-06 |
| [`land.apc`](land.apc.md) | Armoured personnel carrier and MRAP | 2 | road-move, cover | TT-06 |
| [`land.spg`](land.spg.md) | Self-propelled artillery | 2 | move-in, fire, displace, hide | TT-06 |
| [`land.mrl`](land.mrl.md) | Rocket artillery | 2 | move-in, fire, displace | TT-06 |
| [`land.mobile-ad`](land.mobile-ad.md) | Mobile air-defense system | 2 | move, operate | TT-06, TT-09 |
| [`land.ew`](land.ew.md) | Electronic-warfare vehicle | 2 | move, operate | TT-07 |
| [`land.truck`](land.truck.md) | Logistics truck | 1 | convoy, halt | TT-06 |
| [`land.ugv`](land.ugv.md) | Uncrewed ground vehicle | 1 | task | TT-06 |

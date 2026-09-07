#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Render the test-track documents from the YAML sources (plan 07).

Reads catalogue.yaml (and its includes), classes.yaml, sensors.yaml, scenarios.yaml and
writes vehicle-catalogue.md, platforms/<id>.md, class-profiles/README.md and
class-profiles/<class>.md, sensor-models.md, scenario-library.md. Refuses to render a
platform whose figures have no source or no confidence mark (sourcing-and-legal.md §8).

Usage (from the workspace root):

    python docs/test-tracks/tools/build_catalogue.py          # render
    python docs/test-tracks/tools/build_catalogue.py --check  # check only

Requires PyYAML. Exit status is non-zero when a check fails.
"""
from __future__ import annotations

import sys
from collections import defaultdict
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
TT = HERE.parent
NL = "\n"
DATE = "2026-09-04"
ENVELOPE_FIELDS = ["speed_mps", "altitude_m", "climb_mps", "turn_g", "endurance_h", "range_km"]


def load_yaml(p: Path):
    return yaml.safe_load(p.read_text(encoding="utf-8"))


def load_all():
    cat = load_yaml(TT / "catalogue.yaml")
    platforms, domains = [], {}
    for inc in cat["includes"]:
        d = load_yaml(TT / inc)
        for p in d["platforms"]:
            p["_domain"] = d["domain"]
            platforms.append(p)
        domains[d["domain"]] = d
    classes = load_yaml(TT / "classes.yaml")
    sensors = load_yaml(TT / "sensors.yaml")
    scenarios = load_yaml(TT / "scenarios.yaml")
    return cat, platforms, domains, classes, sensors, scenarios


def write(p: Path, s: str) -> None:
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(s if s.endswith(NL) else s + NL, encoding="utf-8", newline=NL)


def rng(v) -> str:
    if isinstance(v, list) and len(v) == 2:
        return f"{v[0]} to {v[1]}" if v[0] != v[1] else f"{v[0]}"
    return str(v)


# ----------------------------------------------------------------------------- checks
def check(platforms, classes, sensors, scenarios) -> list[str]:
    problems = []
    class_ids = {c["id"] for c in classes["classes"]}
    by_class = defaultdict(list)
    for p in platforms:
        if p["class"] not in class_ids:
            problems.append(f"{p['id']}: unknown class {p['class']}")
        if not p.get("sources"):
            problems.append(f"{p['id']}: no sources")
        if p.get("confidence") not in ("high", "medium", "low", "unknown"):
            problems.append(f"{p['id']}: confidence missing or invalid")
        for f in ENVELOPE_FIELDS:
            if f not in p:
                problems.append(f"{p['id']}: missing {f}")
        by_class[p["class"]].append(p)
    for c in classes["classes"]:
        env = c["envelope"]
        for p in by_class.get(c["id"], []):
            for f in ("speed_mps", "altitude_m"):
                lo, hi = env[f]
                plo, phi = p[f]
                if plo < lo or phi > hi:
                    problems.append(f"{p['id']}: {f} {p[f]} outside class envelope {env[f]} of {c['id']}")
            if "dash_speed_mps" in p and p["dash_speed_mps"][1] > env["speed_mps"][1]:
                problems.append(f"{p['id']}: dash speed outside class envelope")
        if not by_class.get(c["id"]):
            problems.append(f"{c['id']}: no platform entry")
        for ph in c["phases"]:
            if ph["speed_mps"][1] > env["speed_mps"][1] or ph["speed_mps"][0] < env["speed_mps"][0]:
                problems.append(f"{c['id']}: phase {ph['name']} speed outside the envelope")
    sensor_types = {s["id"] for s in sensors["types"]}
    for c in classes["classes"]:
        for s in c["sensors"]:
            if s not in sensor_types:
                problems.append(f"{c['id']}: unknown sensor type {s}")
    for name, sset in scenarios["sensor_sets"].items():
        for s in sset:
            if s["type"] not in sensor_types:
                problems.append(f"sensor set {name}: unknown type {s['type']}")
    vignettes = set()
    for sc in scenarios["scenarios"]:
        vignettes.add(sc["vignette"])
        if sc.get("entities") != "inherit":
            for e in sc["entities"]:
                if e["class"] not in class_ids:
                    problems.append(f"{sc['id']}: unknown class {e['class']}")
                if not any(p["id"] == e["platform"] for p in platforms):
                    problems.append(f"{sc['id']}: unknown platform {e['platform']}")
        for s in sc["sensors"]:
            if s not in scenarios["sensor_sets"]:
                problems.append(f"{sc['id']}: unknown sensor set {s}")
    for n in range(1, 11):
        if f"VG-{n:02d}" not in vignettes:
            problems.append(f"vignette VG-{n:02d} has no scenario")
    return problems


# ----------------------------------------------------------------------------- render
def render_catalogue(cat, platforms, domains, classes):
    cname = {c["id"]: c["name"] for c in classes["classes"]}
    L = ["# Vehicle catalogue", "",
         f"Status: rendered by `tools/build_catalogue.py` from `catalogue-*.yaml` (version {cat['version']}) on {DATE}; do not edit by hand. "
         "One table per domain; per-platform detail pages with sources under `platforms/`. Every figure follows `sourcing-and-legal.md`; "
         "figures are published approximations organised by kinematic class for tracker testing, given as ranges. Speeds in m/s, altitudes in m, endurance in hours, range in km.",
         ""]
    for dom in ("air", "sea", "land"):
        d = domains[dom]
        L.append(f"## {dom.capitalize()} (reviewer: {d.get('reviewer', 'pending')})")
        L.append("")
        L.append("| Class | Platform | Side | Speed | Altitude (typical) | Endurance | Range | RCS | IR | Acoustic | Confidence |")
        L.append("|---|---|---|---|---|---|---|---|---|---|---|")
        for p in [x for x in platforms if x["_domain"] == dom]:
            alt = rng(p["altitude_m"]) + (f" ({rng(p['typical_altitude_m'])})" if p.get("typical_altitude_m") and p["altitude_m"] != [0, 0] else "")
            L.append(f"| {cname[p['class']]} | [{p['name']}](platforms/{p['id']}.md) | {p['side']} | {rng(p['speed_mps'])} | {alt} | {rng(p['endurance_h'])} | {rng(p['range_km'])} | {p['rcs_class']} | {p['ir_class']} | {p['acoustic_class']} | {p['confidence']} |")
        L.append("")
    L.append("## Validation record")
    L.append("")
    L.append("| Date | Reviewer | Domain | Fields checked | Outcome |")
    L.append("|---|---|---|---|---|")
    L.append(f"| {DATE} | Drafting agent | all | sources present, confidence present, envelope containment (`tools/build_catalogue.py --check`) | first draft; no subject-matter review yet |")
    L.append("")
    L.append("## Change log")
    L.append("")
    L.append(f"- {DATE}: first draft, {len(platforms)} platform entries in {len(classes['classes'])} classes.")
    write(TT / "vehicle-catalogue.md", NL.join(L))

    for p in platforms:
        L = [f"# {p['name']}", "",
             f"Class: [{cname[p['class']]}](../class-profiles/{p['class']}.md) (`{p['class']}`) · side: {p['side']} · role: {p['role']} · confidence: **{p['confidence']}**", "",
             "| Figure | Value |", "|---|---|"]
        for f, label in (("speed_mps", "Speed (m/s)"), ("dash_speed_mps", "Dash speed (m/s)"), ("cross_country_mps", "Cross-country speed (m/s)"),
                         ("altitude_m", "Altitude (m)"), ("typical_altitude_m", "Typical altitude (m)"), ("climb_mps", "Climb (m/s)"),
                         ("turn_g", "Lateral acceleration (g)"), ("endurance_h", "Endurance (h)"), ("range_km", "Range (km)")):
            if f in p:
                L.append(f"| {label} | {rng(p[f])} |")
        L.append(f"| Radar cross-section class | {p['rcs_class']} |")
        L.append(f"| Infrared class | {p['ir_class']} |")
        L.append(f"| Acoustic class | {p['acoustic_class']} |")
        L.append(f"| Emissions | {p['emissions']} |")
        L.append("")
        L.append("## Sources")
        L.append("")
        for s in p["sources"]:
            note = f" Note: {s['note']}." if s.get("note") else ""
            L.append(f"- {s['ref']} (fields: {', '.join(s['fields'])}).{note}")
        L.append("")
        L.append(f"Rendered from `../catalogue-{p['_domain']}.yaml` by `tools/build_catalogue.py`; policy `../sourcing-and-legal.md`.")
        write(TT / "platforms" / f"{p['id']}.md", NL.join(L))


def render_classes(classes, platforms):
    by_class = defaultdict(list)
    for p in platforms:
        by_class[p["class"]].append(p)
    L = ["# Class profiles", "",
         f"Status: rendered by `tools/build_catalogue.py` from `../classes.yaml` (version {classes['version']}) on {DATE}. "
         "One file per kinematic class: the envelope every platform in the class fits, the phases of a representative mission with the movement model per phase, "
         "the randomization rules, signatures, the sensors that see the class, and the threads and scenarios that use it. The movement models are implemented in `../tools/gen_tracks.py` "
         "and are the specification for the `gungnir-scenario` generator (GAP-016).", "",
         "| Class | Name | Platforms | Phases | Scenarios |", "|---|---|---|---|---|"]
    for c in classes["classes"]:
        L.append(f"| [`{c['id']}`]({c['id']}.md) | {c['name']} | {len(by_class[c['id']])} | {', '.join(ph['name'] for ph in c['phases'])} | {', '.join(c['scenarios'])} |")
    write(TT / "class-profiles" / "README.md", NL.join(L))
    for c in classes["classes"]:
        env = c["envelope"]
        L = [f"# {c['name']} (`{c['id']}`)", "", c["description"], "",
             "## Envelope", "",
             "| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |", "|---|---|---|---|---|",
             f"| {rng(env['speed_mps'])} | {rng(env['altitude_m'])} | {rng(env['climb_mps'])} | {rng(env['turn_g'])} | {rng(env['accel_mps2'])} |", "",
             "## Phases of a representative mission", "",
             "| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |", "|---|---|---|---|---|---|"]
        for ph in c["phases"]:
            extra = {k: v for k, v in ph.items() if k not in ("name", "model", "duration_s", "speed_mps", "altitude_m")}
            L.append(f"| {ph['name']} | `{ph['model']}` | {rng(ph['duration_s'])} | {rng(ph['speed_mps'])} | {rng(ph['altitude_m'])} | {', '.join(f'{k}={rng(v)}' for k, v in extra.items()) or ''} |")
        L.append("")
        L.append("## Randomization")
        L.append("")
        for k, v in c["randomization"].items():
            L.append(f"- {k}: {v}")
        L.append("")
        L.append("## Signatures and sensors")
        L.append("")
        sig = c["signatures"]
        L.append(f"Radar cross-section {sig['rcs']}, infrared {sig['ir']}, acoustic {sig['acoustic']}. Seen by: {', '.join('`' + s + '`' for s in c['sensors'])} (`../sensor-models.md`).")
        L.append("")
        L.append("## Platforms in this class")
        L.append("")
        for p in by_class[c["id"]]:
            L.append(f"- [{p['name']}](../platforms/{p['id']}.md) ({p['side']}, confidence {p['confidence']})")
        L.append("")
        L.append(f"## Threads and scenarios")
        L.append("")
        L.append(f"Threads {', '.join(c['threads'])}; scenarios {', '.join(c['scenarios'])} (`../scenario-library.md`).")
        L.append("")
        L.append(f"Rendered from `../classes.yaml` by `tools/build_catalogue.py`.")
        write(TT / "class-profiles" / f"{c['id']}.md", NL.join(L))


def render_sensors(sensors):
    L = ["# Sensor models", "",
         f"Status: rendered by `tools/build_catalogue.py` from `sensors.yaml` (version {sensors['version']}) on {DATE}. "
         "The observation models that turn truth into `DetectionView`s in `tools/gen_tracks.py`. Every value is an engineering assumption for tracker testing, "
         "not any real sensor's performance (`sourcing-and-legal.md` §4); scenarios override per instance.", "",
         "## How a detection is produced", "",
         "1. Every `update_period_s` (with a per-sensor phase offset) the sensor has an opportunity against every alive, unoccluded entity.",
         "2. The entity's signature class for the sensor's `signature_key` selects the range band; beyond it there is no detection. Cooperative sensors (AIS, ADS-B, IFF, blue-force tracking) see only entities that emit the matching signal; `moving_only` sensors skip stationary entities; `cued` sensors need another sensor to have detected the entity within the last 10 s; a radar `horizon` limits range by the radar-horizon rule for the two heights.",
         "3. Inside range and the field of regard and altitude limits, the detection happens with probability `pd_in_range × (1 − dropout × ea.dropout_multiplier)`.",
         "4. The measurement is the true position plus Gaussian noise along the line of sight (`range_m`), across it (`cross_m`), and vertically (`height_m`), plus any instance `bias_m` (the registration test) and, for a spoofed AIS, the spoof offset.",
         "5. `source_time` is the opportunity time minus any electronic-attack clock skew (a lagging clock; a leading one would be quarantined by the gateway rule that source time may not lead receipt time by more than one second). `receipt_time` is `source_time` plus `latency_s.mean` plus a half-normal jitter, plus an extra 1 to 4 s with probability `out_of_order`, so late data arrives across sensors as the tracking core must handle (`gungnir-time`).",
         "6. False alarms: Poisson(`false_alarms_per_scan × ea.fa_multiplier`) per opportunity, uniform within the sensor's largest range band and altitude limits.",
         "7. Events change the parameters in time: `sensor_lost` stops a sensor, `ea_skew` and `ea_dropout` apply the electronic-attack multipliers to the named sensors, `sea_state` raises coastal-radar dropout.", "",
         "## Types", "",
         "| Type | Name | Range by class (m) | Pd | Period (s) | Noise range / cross / height (m) | Field of regard (deg) | Altitude (m) | Latency mean ± jitter (s) | Dropout | Out of order | False alarms per scan | Traits |",
         "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for t in sensors["types"]:
        traits = [k for k in ("horizon", "cued", "moving_only", "cooperative", "mobile", "spoofable", "gnss_dependent") if t.get(k)]
        rb = ", ".join(f"{k} {v}" for k, v in t["range_m"].items())
        n = t["noise"]
        L.append(f"| `{t['id']}` | {t['name']} | {rb} | {t['pd_in_range']} | {t['update_period_s']} | {n['range_m']} / {n['cross_m']} / {n['height_m']} | {rng(t['field_of_regard_deg'])} | {rng(t['altitude_m'])} | {t['latency_s']['mean']} ± {t['latency_s']['jitter']} | {t['dropout']} | {t['out_of_order']} | {t['false_alarms_per_scan']} | {', '.join(traits)} |")
    L.append("")
    L.append("## Electronic-attack sensitivities")
    L.append("")
    L.append("| Type | Clock skew (s) when attacked | Dropout multiplier | False-alarm multiplier |")
    L.append("|---|---|---|---|")
    for t in sensors["types"]:
        ea = t["ea"]
        L.append(f"| `{t['id']}` | {ea['skew_s']} | {ea['dropout_multiplier']} | {ea['fa_multiplier']} |")
    L.append("")
    L.append("## Alignment with the tracking core")
    L.append("")
    L.append("- Latency, jitter, and the out-of-order fraction produce the multi-rate, out-of-sequence arrival the `gungnir-fusion-async` pipeline is built for (Scenario 3 of `../scenario-crate-narrative.md`).")
    L.append("- Coastal-radar false alarms at sea state 4 produce the clutter of Scenario 2; the acoustic network's coarse, late reports produce the low-quality source Scenario 3 mixes in.")
    L.append("- The ISR-video bias in TT-06 is the registration test of the `gungnir-track-fusion` rows; the ground truth of the bias is in the set's `sensors.json`.")
    L.append("- Cooperative sensors carry identity evidence for `gungnir-identification` (GAP-010) without any classification appearing in the observation stream.")
    L.append("")
    L.append("Rendered from `sensors.yaml` by `tools/build_catalogue.py`.")
    write(TT / "sensor-models.md", NL.join(L))


def render_scenarios(scenarios, classes):
    cname = {c["id"]: c["name"] for c in classes["classes"]}
    L = ["# Scenario library", "",
         f"Status: rendered by `tools/build_catalogue.py` from `scenarios.yaml` (version {scenarios['version']}) on {DATE}. "
         "One or more scenarios per mission vignette, composed from the class profiles and sensor models on the fictional Vell estuary; each names its classes, counts, timing, sensors, events, and the expected outcome the validation and the measures check. "
         "`sample` gives the reduced composition committed under `../../testdata/tracks/samples/`; full-size sets are generated on demand.", "",
         "| Scenario | Vignette | Thread | Entities (full) | Duration (full) | Sensor sets | Sample |", "|---|---|---|---|---|---|---|"]
    by_id = {s["id"]: s for s in scenarios["scenarios"]}
    for s in scenarios["scenarios"]:
        ents = s["entities"] if s.get("entities") != "inherit" else by_id[s["base"]]["entities"]
        n = sum(e["count"] for e in ents)
        smp = s["sample"]
        L.append(f"| [{s['id']}](#{s['id'].lower()}) {s['name']} | {s['vignette']} | {s['thread']} | {n} | {s['duration_s']} s | {', '.join(s['sensors'])} | {smp['duration_s']} s, scale {smp['entity_scale']}, seed {smp['seed']} |")
    L.append("")
    for s in scenarios["scenarios"]:
        L.append(f"## {s['id']} {s['name']}")
        L.append("")
        L.append(f"Vignette {s['vignette']}, thread {s['thread']}, start {s['start_time_of_day']}, duration {s['duration_s']} s" + (f", based on {s['base']}" if s.get("base") else "") + ".")
        L.append("")
        L.append(s["narrative"])
        L.append("")
        ents = s["entities"] if s.get("entities") != "inherit" else by_id[s["base"]]["entities"]
        L.append("| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |")
        L.append("|---|---|---|---|---|---|---|---|")
        for e in ents:
            notes = ", ".join(f"{k}={v}" for k, v in e.items() if k not in ("group", "class", "platform", "side", "count", "spawn_s", "route"))
            L.append(f"| {e['group']} | {cname[e['class']]} | {e['platform']} | {e['side']} | {e['count']} | {rng(e['spawn_s'])} | {e['route']} | {notes} |")
        L.append("")
        if s.get("variants"):
            L.append("Variants: " + "; ".join(f"`{v['id']}` ({', '.join(f'{k}={val}' for k, val in v.items() if k != 'id')})" for v in s["variants"]) + ".")
            L.append("")
        if s.get("events"):
            L.append("Events: " + "; ".join(f"t={ev['t']} {ev['kind']}" + (f" ({ev.get('note')})" if ev.get("note") else "") for ev in s["events"]) + ".")
            L.append("")
        exp = s["expected"]
        L.append("Expected: " + "; ".join(f"{k}: {v}" for k, v in exp.items()) + ".")
        L.append("")
    L.append("## Geography")
    L.append("")
    L.append("Named points (ENU metres from the sector command post): " + "; ".join(f"{k} {v}" for k, v in scenarios["points"].items()) + ".")
    L.append("")
    L.append("Routes: " + "; ".join(f"`{k}` ({len(v)} waypoints)" for k, v in scenarios["routes"].items()) + ".")
    L.append("")
    L.append("Sensor sets: " + "; ".join(f"`{k}` ({', '.join(str(x['id']) + ' ' + x['name'] for x in v)})" for k, v in scenarios["sensor_sets"].items()) + ".")
    L.append("")
    L.append("Rendered from `scenarios.yaml` by `tools/build_catalogue.py`; the vignettes are `../mission/vignettes.md`.")
    write(TT / "scenario-library.md", NL.join(L))


def main(argv):
    cat, platforms, domains, classes, sensors, scenarios = load_all()
    problems = check(platforms, classes, sensors, scenarios)
    for p in problems:
        print("PROBLEM:", p)
    if "--check" not in argv and not problems:
        render_catalogue(cat, platforms, domains, classes)
        render_classes(classes, platforms)
        render_sensors(sensors)
        render_scenarios(scenarios, classes)
        print(f"rendered {len(platforms)} platforms, {len(classes['classes'])} classes, {len(sensors['types'])} sensor types, {len(scenarios['scenarios'])} scenarios")
    print(f"catalogue check: {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

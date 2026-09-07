#!/usr/bin/env python3
"""Reference generator for test-track sets (plan 07).

Reads classes.yaml, sensors.yaml, scenarios.yaml, and the catalogue, and writes a set
(metadata.json, truth.jsonl, detections.jsonl, sensors.json, events.jsonl) per
docs/test-tracks/data-format.md. Deterministic: the same inputs and seed give
byte-identical output. This is the specification the gungnir-scenario generator
(GAP-016) must reproduce; it is written for clarity, not speed.

Usage (from the workspace root):

    python docs/test-tracks/tools/gen_tracks.py                 # every scenario, sample variant
    python docs/test-tracks/tools/gen_tracks.py TT-01 TT-04     # selected scenarios, sample
    python docs/test-tracks/tools/gen_tracks.py --full TT-01    # full size into testdata/tracks/full/

Requires PyYAML.
"""
from __future__ import annotations

import json
import math
import random
import sys
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
TT = HERE.parent
ROOT = TT.parent.parent
GENERATOR = "tt-gen 0.1.0"
G = 9.80665
# The recorded adapter releases a detection when mission time reaches its source time,
# and the gateway then rejects a receipt time more than MAX_FUTURE_RECEIPT_S (5 s) ahead
# of now. So receipt minus source must stay under that window, and an electronic-attack
# clock skew counts against it: a lagging clock reports an older source time, which makes
# the apparent latency larger. The scenarios' skew is small enough that skewed data still
# reaches the tracker, which is the point of TT-07: the skew must be detected (MOP-09),
# not silently quarantined.
MAX_LATENCY_S = 4.9
# Clock skew from electronic attack is applied as a lagging clock (source time behind
# the true observation time); a leading clock would be quarantined by the gateway.


def load(name):
    return yaml.safe_load((TT / name).read_text(encoding="utf-8"))


def norm(v):
    return math.sqrt(sum(x * x for x in v))


def sub(a, b):
    return [a[i] - b[i] for i in range(3)]


def add(a, b):
    return [a[i] + b[i] for i in range(3)]


def scale(a, k):
    return [x * k for x in a]


def unit(v):
    n = norm(v)
    return [x / n for x in v] if n > 1e-9 else [0.0, 0.0, 0.0]


def draw(rng, r):
    return rng.uniform(r[0], r[1]) if isinstance(r, list) else float(r)


class Entity:
    def __init__(self, eid, spec, cls, platform, route, spawn, phases, rng, extra):
        self.id = eid
        self.spec = spec
        self.cls = cls
        self.platform = platform
        self.side = spec["side"]
        self.route = route
        self.spawn = spawn
        self.phases = phases
        self.phase_i = 0
        self.phase_t = 0.0
        self.pos = list(route[0]) if route else list(extra.get("at", [0, 0, 0]))
        self.vel = [0.0, 0.0, 0.0]
        self.heading = 0.0
        self.alive = True
        self.spawned = False
        self.wp = 1
        self.decoy = bool(spec.get("decoy"))
        self.no_terminal = bool(spec.get("no_terminal"))
        self.occluded_window = spec.get("cover_gap_s")
        self.emitting = spec.get("emitting", True)
        self.flags = extra
        self.orbit_centre = None
        self.orbit_dir = 1 if rng.random() < 0.5 else -1
        self.stopped_until = -1.0
        self.fired = False
        self.target_entity = None
        self.speed = 0.0
        self.alt_target = 0.0
        self.phase_params = {}
        self.rng = rng
        self._enter_phase()

    def phase(self):
        return self.phases[self.phase_i] if self.phase_i < len(self.phases) else None

    def _enter_phase(self):
        ph = self.phase()
        if ph is None:
            self.alive = False
            return
        self.phase_t = 0.0
        self.phase_params = {
            "duration": draw(self.rng, ph["duration_s"]),
            "speed": draw(self.rng, ph["speed_mps"]),
            "alt": draw(self.rng, ph["altitude_m"]),
            "radius": draw(self.rng, ph.get("radius_m", [1000, 1000])),
            "weave": ph.get("weave", 0.0),
            "stop_prob": ph.get("stop_prob", 0.0),
            "fire_stop": draw(self.rng, ph.get("fire_stop_s", [120, 120])),
            "glide_ratio": draw(self.rng, ph.get("glide_ratio", [7, 7])),
            "apogee": draw(self.rng, ph.get("apogee_m", [40000, 40000])),
        }
        self.speed = self.phase_params["speed"]
        if ph["model"] in ("orbit", "hover", "anchored", "stationary", "shoot-and-move"):
            self.orbit_centre = list(self.pos)
        if ph["model"] == "orbit" and "at" in self.flags:
            self.orbit_centre = list(self.flags["at"])
            self.pos = add(self.orbit_centre, [self.phase_params["radius"], 0.0, 0.0])

    def next_phase(self):
        self.phase_i += 1
        self._enter_phase()

    def is_occluded(self, t):
        w = self.occluded_window
        return bool(w) and w[0] <= t - self.spawn <= w[1]

    def step(self, t, dt, targets):
        ph = self.phase()
        if ph is None or not self.alive:
            return
        env = self.cls["envelope"]
        model = ph["model"]
        p = self.phase_params
        max_turn = max(env["turn_g"]) * G / max(self.speed, 1.0)  # rad/s
        if model in ("waypoint-cruise", "nap-of-earth", "road-move", "sea-transit", "evade"):
            if self.wp >= len(self.route):
                if self.no_terminal:
                    self.pos = add(self.pos, scale(self.vel, dt))
                    self.phase_t += dt
                    if self.phase_t > p["duration"]:
                        self.alive = False
                    return
                self.next_phase()
                if self.phase() is None:
                    self.alive = False
                return
            if model == "road-move" and t >= self.stopped_until and self.rng.random() < p["stop_prob"] * dt / 60.0:
                self.stopped_until = t + self.rng.uniform(30, 180)
            if t < self.stopped_until:
                self.vel = [0.0, 0.0, 0.0]
                self.phase_t += dt
                return
            target = self.route[self.wp]
            to = sub(target, self.pos)
            to[2] = 0.0
            dist = norm(to)
            if dist < max(self.speed * dt * 1.5, 30.0):
                self.wp += 1
                if self.wp >= len(self.route):
                    return
                target = self.route[self.wp]
                to = sub(target, self.pos)
                to[2] = 0.0
            desired = math.atan2(to[1], to[0])
            if model == "evade":
                desired = self.heading + max_turn * dt * self.orbit_dir * 3
            delta = (desired - self.heading + math.pi) % (2 * math.pi) - math.pi
            delta = max(-max_turn * dt, min(max_turn * dt, delta))
            self.heading += delta
            if p["weave"]:
                self.heading += self.rng.gauss(0.0, p["weave"]) * dt
            spd = self.speed * (1.0 + (self.rng.gauss(0.0, 0.03) if model in ("road-move", "sea-transit") else 0.0))
            spd = min(max(spd, env["speed_mps"][0]), env["speed_mps"][1])
            self.vel = [spd * math.cos(self.heading), spd * math.sin(self.heading), 0.0]
            alt_target = p["alt"] + (self.rng.gauss(0.0, 8.0) if model == "nap-of-earth" else 0.0)
            climb = max(-abs(env["climb_mps"][0]), min(env["climb_mps"][1], (alt_target - self.pos[2]) / max(dt, 1.0)))
            if env["altitude_m"] == [0, 0]:
                climb = 0.0
            self.vel[2] = climb
            self.pos = add(self.pos, scale(self.vel, dt))
            self.pos[2] = max(0.0, self.pos[2])
            self.phase_t += dt
            # terminal trigger for classes that end on the target
            if self.phase_i < len(self.phases) - 1 and self.phases[-1]["model"] == "dash-terminal" and not self.no_terminal:
                final = self.route[-1]
                if norm(sub([final[0], final[1], 0.0], [self.pos[0], self.pos[1], 0.0])) < 3000.0:
                    self.phase_i = len(self.phases) - 1
                    self._enter_phase()
        elif model == "orbit":
            r = max(p["radius"], 50.0)
            omega = self.orbit_dir * self.speed / r
            ang = math.atan2(self.pos[1] - self.orbit_centre[1], self.pos[0] - self.orbit_centre[0]) + omega * dt
            new = [self.orbit_centre[0] + r * math.cos(ang), self.orbit_centre[1] + r * math.sin(ang), self.pos[2]]
            alt_target = p["alt"] if env["altitude_m"] != [0, 0] else 0.0
            new[2] = self.pos[2] + max(-abs(env["climb_mps"][0]), min(env["climb_mps"][1], (alt_target - self.pos[2]) / max(dt, 1.0))) * dt
            self.vel = scale(sub(new, self.pos), 1.0 / dt)
            self.heading = math.atan2(self.vel[1], self.vel[0])
            self.pos = new
            self.phase_t += dt
            if self.phase_t > p["duration"]:
                self.next_phase()
        elif model in ("hover", "anchored", "stationary"):
            drift = 0.3 if model != "stationary" else 0.0
            self.vel = [self.rng.gauss(0.0, drift), self.rng.gauss(0.0, drift), 0.0]
            self.pos = add(self.pos, scale(self.vel, dt))
            if env["altitude_m"] != [0, 0] and model == "hover":
                self.pos[2] += (p["alt"] - self.pos[2]) * min(1.0, dt / 10.0)
            self.phase_t += dt
            if self.phase_t > p["duration"]:
                self.next_phase()
        elif model == "shoot-and-move":
            self.vel = [0.0, 0.0, 0.0]
            self.phase_t += dt
            if not self.fired and self.phase_t >= p["fire_stop"] * 0.5:
                self.fired = True
                self.flags["fired_at"] = t
            if self.phase_t > p["fire_stop"]:
                # displace: a short random route away from the firing position
                ang = self.rng.uniform(0, 2 * math.pi)
                dist = self.rng.uniform(800, 2000)
                self.route = [list(self.pos), [self.pos[0] + dist * math.cos(ang), self.pos[1] + dist * math.sin(ang), 0.0]]
                self.wp = 1
                self.next_phase()
        elif model == "dash-terminal":
            if self.target_entity is not None and self.target_entity.alive:
                tgt = self.target_entity.pos
            else:
                tgt = self.flags.get("target") or (self.route[-1] if self.route else self.pos)
            to = sub(tgt, self.pos)
            dist = norm(to)
            dash = max(self.speed, p["speed"])
            if dist <= dash * dt:
                self.pos = list(tgt)
                self.alive = False
                self.flags["impact_at"] = t
                if self.target_entity is not None and self.target_entity.alive:
                    self.target_entity.alive = False
                    self.target_entity.flags["destroyed_at"] = t
                return
            self.vel = scale(unit(to), dash)
            self.heading = math.atan2(self.vel[1], self.vel[0])
            self.pos = add(self.pos, scale(self.vel, dt))
            self.phase_t += dt
        elif model == "glide":
            final = self.route[-1]
            to = sub(final, self.pos)
            horiz = norm([to[0], to[1], 0.0])
            if horiz <= self.speed * dt or self.pos[2] <= 0.0:
                self.alive = False
                self.flags["impact_at"] = t
                return
            self.vel = scale(unit([to[0], to[1], 0.0]), self.speed)
            self.vel[2] = -self.speed / p["glide_ratio"]
            self.pos = add(self.pos, scale(self.vel, dt))
            self.phase_t += dt
        elif model == "ballistic":
            final = self.route[-1]
            start = self.route[0]
            total = norm(sub([final[0], final[1], 0.0], [start[0], start[1], 0.0]))
            frac = min(1.0, self.phase_t / p["duration"])
            x = [start[0] + (final[0] - start[0]) * frac, start[1] + (final[1] - start[1]) * frac, 4 * p["apogee"] * frac * (1 - frac)]
            self.vel = scale(sub(x, self.pos), 1.0 / dt)
            self.pos = x
            self.phase_t += dt
            if frac >= 1.0:
                self.alive = False
                self.flags["impact_at"] = t
        else:
            raise ValueError(f"unknown model {model}")


def resolve_point(pts, v):
    if isinstance(v, str):
        return list(pts[v])
    return [float(x) for x in v]


def jitter_route(rng, route, lateral):
    out = []
    for i, w in enumerate(route):
        if i in (0, len(route) - 1):
            out.append(list(w))
        else:
            out.append([w[0] + rng.uniform(-lateral, lateral), w[1] + rng.uniform(-lateral, lateral), w[2]])
    return out


def build_scenario(scen, scenarios, classes, platforms, sensors_yaml, full, seed_override=None, variant=None):
    pts = scenarios["points"]
    routes = {k: [resolve_point(pts, w) for w in v] for k, v in scenarios["routes"].items()}
    base = scen
    ents_spec = scen["entities"]
    if ents_spec == "inherit":
        base = next(s for s in scenarios["scenarios"] if s["id"] == scen["base"])
        ents_spec = base["entities"]
    sample = scen["sample"]
    duration = scen["duration_s"] if full else sample["duration_s"]
    scale_n = 1.0 if full else sample["entity_scale"]
    seed = seed_override if seed_override is not None else (sample["seed"] if not full else sample["seed"] + 1000)
    sensor_sets = scen["sensors"] if full else sample.get("sensors", scen["sensors"])
    events = scen.get("events", []) if full else sample.get("events", scen.get("events", []))
    tick = scenarios["truth_tick_s"] if full else 2.0
    rng = random.Random(seed)
    cls_by = {c["id"]: c for c in classes["classes"]}
    plat_by = {p["id"]: p for p in platforms}

    # sensors
    sensor_instances = []
    for sname in sensor_sets:
        for s in scenarios["sensor_sets"][sname]:
            t = next(x for x in sensors_yaml["types"] if x["id"] == s["type"])
            inst = {"id": s["id"], "type": s["type"], "name": s["name"], "pos": resolve_point(pts, s["pos"]),
                    "params": {k: v for k, v in t.items() if k not in ("id", "name", "confidence")},
                    "calibration": s.get("calibration"), "phase": rng.uniform(0, t["update_period_s"])}
            for k, v in (s.get("overrides") or {}).items():
                inst["params"][k] = v
            sensor_instances.append(inst)
    var = variant if variant else (sample.get("variant") if not full else None)
    if var and scen.get("variants"):
        v = next(x for x in scen["variants"] if x["id"] == var)
        for mv in v.get("moves", []):
            for inst in sensor_instances:
                if inst["id"] == mv["sensor"]:
                    inst["pos"] = resolve_point(pts, mv["pos"])

    # entities
    entities = []
    spawn_cap = duration * 0.7
    for spec in ents_spec:
        cls = cls_by[spec["class"]]
        plat = plat_by[spec["platform"]]
        count = max(1, int(round(spec["count"] * scale_n)))
        phases = cls["phases"]
        if spec.get("phases"):
            phases = [ph for ph in cls["phases"] if ph["name"] in spec["phases"]]
        lateral = 2000.0 if cls["domain"] == "air" else (300.0 if cls["domain"] == "sea" else 20.0)
        for i in range(count):
            eid = f"{scen['id'].replace('-', '')}-{spec['group']}-{i + 1:03d}"
            spawn_lo, spawn_hi = spec["spawn_s"]
            spawn = rng.uniform(min(spawn_lo, spawn_cap), min(spawn_hi, spawn_cap)) if spawn_hi > 0 else 0.0
            route = []
            if spec.get("route", "none") != "none":
                route = [list(w) for w in routes[spec["route"]]]
                if spec.get("reverse"):
                    route = list(reversed(route))
                route = jitter_route(rng, route, lateral)
                if spec.get("spacing_m"):
                    off = i * float(spec["spacing_m"])
                    route = [[w[0] - off * 0.7, w[1] - off * 0.7, w[2]] for w in route]
            extra = {}
            if spec.get("at"):
                at = resolve_point(pts, spec["at"])
                if spec.get("spacing_m"):
                    at = [at[0] + i * spec["spacing_m"], at[1], at[2]]
                extra["at"] = at
            if spec.get("target"):
                extra["target"] = resolve_point(pts, spec["target"])
            if spec.get("launch"):
                route = [resolve_point(pts, spec["launch"])]
            for k in ("adsb", "iff", "ais", "adsb_intermittent", "ais_spoof_offset_m", "target_group", "launch"):
                if k in spec:
                    extra[k] = spec[k]
            e = Entity(eid, spec, cls, plat, route, spawn, phases, rng, extra)
            entities.append(e)
    return {"scenario": scen, "duration": duration, "seed": seed, "tick": tick, "rng": rng, "sensors": sensor_instances,
            "entities": entities, "events": events, "variant": var, "full": full}


def emission_key(plat, e, sensors_yaml):
    em = plat.get("emissions", "")
    if e.flags.get("adsb"):
        return "adsb"
    if e.flags.get("iff"):
        return "iff"
    if e.flags.get("ais") is False:
        return "none"
    if "AIS" in em:
        return "ais"
    if "blue-force tracking" in em and e.side == "blue":
        return "bft"
    if not e.emitting or em.startswith("none"):
        return "none"
    for key, names in sensors_yaml["emission_map"].items():
        if em in names:
            return key
    if "datalink" in em or "control" in em:
        return "datalink"
    if "radar" in em:
        return "radar"
    return "none"


def radar_horizon_m(h1, h2):
    return 4120.0 * (math.sqrt(max(h1, 0.0)) + math.sqrt(max(h2, 0.0)))


def run(built, classes, sensors_yaml, out_dir: Path):
    scen = built["scenario"]
    rng = built["rng"]
    dt = built["tick"]
    duration = built["duration"]
    entities = built["entities"]
    sensors = built["sensors"]
    events = built["events"]
    truth, detections, event_lines = [], [], []
    last_seen = {}  # entity id -> time last detected by a non-cued sensor
    active_ea = {}
    lost = set()
    sea_state = 0
    for ev in events:
        event_lines.append(dict(ev))
    # assign interceptor targets
    for e in entities:
        if e.flags.get("target_group"):
            cands = [x for x in entities if x.spec["group"] == e.flags["target_group"]]
            if cands:
                e.target_entity = cands[len([x for x in entities if x.flags.get("target_group") == e.flags["target_group"] and x is not e]) % len(cands)]
    next_scan = {s["id"]: s["phase"] for s in sensors}
    t = 0.0
    fa_total = 0
    while t <= duration + 1e-9:
        # events
        for ev in events:
            if abs(ev["t"] - t) < dt / 2:
                if ev["kind"] == "sensor_lost":
                    lost.add(ev["sensor"])
                elif ev["kind"] == "sensor_restored":
                    lost.discard(ev["sensor"])
                elif ev["kind"] in ("ea_skew", "ea_dropout"):
                    for sid in ev["sensors"]:
                        active_ea.setdefault(sid, {})[ev["kind"]] = (ev, ev.get("until", duration))
                elif ev["kind"] == "sea_state":
                    sea_state = ev["value"]
        # entities
        for e in entities:
            if not e.spawned and t >= e.spawn:
                e.spawned = True
            if not e.spawned or not e.alive:
                continue
            e.step(t, dt, entities)
            if e.fired and e.flags.get("fired_at") == t:
                event_lines.append({"t": t, "kind": "fires", "entity": e.id})
            truth.append({"t": round(t, 3), "entity": e.id, "class": e.cls["id"], "platform": e.platform["id"], "side": e.side,
                          "phase": e.phase()["name"] if e.phase() else "ended", "pos": [round(x, 1) for x in e.pos],
                          "vel": [round(x, 2) for x in e.vel], "alive": e.alive, "occluded": e.is_occluded(t)})
        # sensors
        for s in sensors:
            if s["id"] in lost:
                continue
            p = s["params"]
            while next_scan[s["id"]] <= t:
                st = next_scan[s["id"]]
                next_scan[s["id"]] += p["update_period_s"]
                ea = active_ea.get(s["id"], {})
                skew = 0.0
                drop_mult, fa_mult = 1.0, 1.0
                if "ea_skew" in ea and st <= ea["ea_skew"][1]:
                    skew = ea["ea_skew"][0].get("skew_s", p["ea"]["skew_s"])
                if "ea_dropout" in ea and st <= ea["ea_dropout"][1]:
                    drop_mult = ea["ea_dropout"][0].get("multiplier", p["ea"]["dropout_multiplier"])
                    fa_mult = p["ea"]["fa_multiplier"]
                dropout = min(0.95, p["dropout"] * drop_mult + (p.get("sea_state_dropout", {}).get(sea_state, 0.0)))
                bands = p["range_m"]
                for e in entities:
                    if not e.spawned or not e.alive or e.is_occluded(st):
                        continue
                    key = p["signature_key"]
                    if key == "rcs":
                        cl = "large" if e.decoy else e.platform["rcs_class"]
                    elif key == "ir":
                        cl = e.platform["ir_class"]
                    elif key == "acoustic":
                        cl = e.platform["acoustic_class"]
                    else:
                        cl = emission_key(e.platform, e, sensors_yaml)
                    rmax = bands.get(cl, 0)
                    if rmax <= 0:
                        continue
                    rel = sub(e.pos, s["pos"])
                    r = norm(rel)
                    if r > rmax:
                        continue
                    if p.get("horizon") and r > radar_horizon_m(s["pos"][2], e.pos[2]):
                        continue
                    lo, hi = p["altitude_m"]
                    if not (lo - 1 <= e.pos[2] <= hi + 1):
                        continue
                    a, b = p["field_of_regard_deg"]
                    bearing = (math.degrees(math.atan2(rel[0], rel[1])) + 360.0) % 360.0
                    if not (a <= bearing <= b or (a > b and (bearing >= a or bearing <= b))) and not (a == 0 and b == 360):
                        continue
                    if p.get("moving_only") and norm(e.vel) < 1.0:
                        continue
                    if p.get("cued") and st - last_seen.get(e.id, -1e9) > 10.0:
                        continue
                    if e.flags.get("adsb_intermittent") and key == "emission" and rng.random() < e.flags["adsb_intermittent"]:
                        continue
                    pd = p["pd_in_range"] * (1.0 - dropout)
                    if rng.random() >= pd:
                        continue
                    los = unit(rel)
                    cross = unit([-los[1], los[0], 0.0])
                    n = p["noise"]
                    meas = add(e.pos, scale(los, rng.gauss(0, n["range_m"])))
                    meas = add(meas, scale(cross, rng.gauss(0, n["cross_m"])))
                    meas[2] += rng.gauss(0, n["height_m"])
                    bias = p.get("bias_m")
                    if bias:
                        meas = add(meas, [bias.get("east", 0.0), bias.get("north", 0.0), 0.0])
                    if e.flags.get("ais_spoof_offset_m") and key == "emission":
                        off = e.flags["ais_spoof_offset_m"]
                        meas = add(meas, [off[0], off[1], 0.0])
                    if e.platform["altitude_m"] == [0, 0]:
                        meas[2] = 0.0
                    source = st - skew
                    latency = p["latency_s"]["mean"] + abs(rng.gauss(0, p["latency_s"]["jitter"]))
                    if rng.random() < p["out_of_order"]:
                        latency += rng.uniform(0.5, 1.5)
                    latency = min(latency, MAX_LATENCY_S - skew)
                    detections.append({"sensor": s["id"], "source_time": round(source, 3), "receipt_time": round(st + latency, 3),
                                       "measurement": [round(x, 2) for x in meas],
                                       "provenance": {"source_sensor_ids": [s["id"]], "calibration_baseline_version": s["calibration"], "algorithm_version": GENERATOR},
                                       "_truth": e.id})
                    if not p.get("cued"):
                        last_seen[e.id] = st
                # false alarms
                lam = p["false_alarms_per_scan"] * fa_mult
                k = 0
                if lam > 0:
                    L = math.exp(-lam)
                    pk = 1.0
                    while True:
                        pk *= rng.random()
                        if pk <= L:
                            break
                        k += 1
                for _ in range(k):
                    rmax = max(bands.values())
                    ang = rng.uniform(0, 2 * math.pi)
                    r = rng.uniform(0.05, 1.0) * rmax
                    lo, hi = p["altitude_m"]
                    z = rng.uniform(lo, min(hi, 5000.0)) if hi > 0 else 0.0
                    meas = [s["pos"][0] + r * math.cos(ang), s["pos"][1] + r * math.sin(ang), z]
                    latency = min(p["latency_s"]["mean"] + abs(rng.gauss(0, p["latency_s"]["jitter"])), MAX_LATENCY_S - skew)
                    detections.append({"sensor": s["id"], "source_time": round(st - skew, 3), "receipt_time": round(st + latency, 3),
                                       "measurement": [round(x, 2) for x in meas],
                                       "provenance": {"source_sensor_ids": [s["id"]], "calibration_baseline_version": s["calibration"], "algorithm_version": GENERATOR},
                                       "_truth": None})
                    fa_total += 1
        t += dt
    detections.sort(key=lambda d: (d["receipt_time"], d["sensor"]))
    # write
    out_dir.mkdir(parents=True, exist_ok=True)
    with (out_dir / "truth.jsonl").open("w", encoding="utf-8", newline="\n") as f:
        for r in truth:
            f.write(json.dumps(r, separators=(",", ":")) + "\n")
    with (out_dir / "detections.jsonl").open("w", encoding="utf-8", newline="\n") as f:
        for d in detections:
            f.write(json.dumps({k: v for k, v in d.items() if k != "_truth"}, separators=(",", ":")) + "\n")
    with (out_dir / "detections-truth.jsonl").open("w", encoding="utf-8", newline="\n") as f:
        for i, d in enumerate(detections):
            f.write(json.dumps({"line": i + 1, "entity": d["_truth"]}, separators=(",", ":")) + "\n")
    with (out_dir / "sensors.json").open("w", encoding="utf-8", newline="\n") as f:
        json.dump({"sensors": [{k: v for k, v in s.items() if k != "phase"} for s in sensors]}, f, indent=1)
    with (out_dir / "events.jsonl").open("w", encoding="utf-8", newline="\n") as f:
        for ev in sorted(event_lines, key=lambda x: x["t"]):
            f.write(json.dumps(ev, separators=(",", ":")) + "\n")
    by_class = {}
    for e in entities:
        by_class[e.cls["id"]] = by_class.get(e.cls["id"], 0) + 1
    meta = {"format": 1, "scenario": scen["id"], "variant": ("full" if built["full"] else "sample") + (f"-{built['variant']}" if built["variant"] else ""),
            "seed": built["seed"], "generator": GENERATOR, "catalogue_version": CAT_VERSION, "classes_version": classes["version"],
            "sensors_version": sensors_yaml["version"], "scenarios_version": SCEN_VERSION, "duration_s": duration, "truth_tick_s": dt,
            "origin": SCEN_ORIGIN, "start_time_of_day": scen["start_time_of_day"],
            "counts": {"entities": len(entities), "entities_by_class": by_class, "truth_records": len(truth), "detections": len(detections),
                       "false_alarms": fa_total, "sensors": len(sensors)},
            "expected": scen.get("expected", {}),
            "provenance": {"policy": "docs/test-tracks/sourcing-and-legal.md", "inputs": ["classes.yaml", "sensors.yaml", "scenarios.yaml", "catalogue-*.yaml"]},
            "validation": {"passed": None, "report": "validation-report.json"}}
    with (out_dir / "metadata.json").open("w", encoding="utf-8", newline="\n") as f:
        json.dump(meta, f, indent=1)
    return meta


CAT_VERSION = None
SCEN_VERSION = None
SCEN_ORIGIN = None


def main(argv):
    global CAT_VERSION, SCEN_VERSION, SCEN_ORIGIN
    full = "--full" in argv
    ids = [a for a in argv if not a.startswith("--")]
    cat = load("catalogue.yaml")
    CAT_VERSION = cat["version"]
    platforms = []
    for inc in cat["includes"]:
        platforms.extend(load(inc)["platforms"])
    classes = load("classes.yaml")
    sensors_yaml = load("sensors.yaml")
    scenarios = load("scenarios.yaml")
    SCEN_VERSION = scenarios["version"]
    SCEN_ORIGIN = scenarios["origin"]
    base_out = ROOT / "testdata" / "tracks" / ("full" if full else "samples")
    for scen in scenarios["scenarios"]:
        if ids and scen["id"] not in ids:
            continue
        built = build_scenario(scen, scenarios, classes, platforms, sensors_yaml, full)
        name = f"{scen['id']}-{'full' if full else 'sample'}"
        meta = run(built, classes, sensors_yaml, base_out / name)
        c = meta["counts"]
        print(f"{name}: {c['entities']} entities, {c['truth_records']} truth, {c['detections']} detections ({c['false_alarms']} false alarms), {c['sensors']} sensors, seed {meta['seed']}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

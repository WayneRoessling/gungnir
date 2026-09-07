# Planning and battle management (cross-cutting function)

Status: first draft, 2026-09-04. Open sources only.

## 1. Function statement

Set the conditions the live missions run under: what is defended and in what order,
where sensors and effectors are placed and what they cover, what the rules of
engagement and authorities are, how the sector rehearses, how it runs its battle
rhythm, and how it learns from each session.

## 2. The defended-asset list

| Element | Content | System representation |
|---|---|---|
| Asset | A place or thing to protect: power station, airfield, port, headquarters, troop concentration, bridge | Protected point or area with priority (extends `gungnir-assessment`, which today has one protected point) |
| Priority | Order of protection when resources are short, from criticality, vulnerability, recuperability, and the threat to it | Priority weight in the reward matrix |
| Status | Active, suspended, moved | Config baseline; changes audited |
| Warning obligations | Who is warned when a threat is inbound, by what means | Alert routing in the workflow |

## 3. Sensor and effector laydown

| Activity | Content | System support |
|---|---|---|
| Coverage planning | Where each sensor sees, given terrain and mode; where the gaps are, especially low-altitude approaches along rivers and coasts | `gungnir-analytics` coverage volumes and line-of-sight over terrain; `gungnir-sensor-management` coverage regions |
| Effector placement | Which layer covers which asset; engagement zones; overlaps | Resource positions and capacities in the config baseline; geofences for engagement zones |
| Readiness | Which effectors are ready, rearming, or down | `ResourceView.ready`; never task an unready resource |
| Re-laydown under attack | Moving sensors and effectors after loss or when the threat axis changes | Coverage before and after; recorded as a baseline change |

## 4. Rules of engagement and authorities

The structure in `air-defense-and-counter-uas.md` §6 is configured here: identification
criteria per class, weapons control status per layer, engagement authority by role
and class, restrictions as geofences and rules, escalation and timeout behaviour. In
the system this is a policy configuration validated before it is applied
(`gungnir-config` validation, `gungnir-policy` engines) and audited on change
(`gungnir-security`). Weapons control status and authority-by-role are not yet
modelled (`ARCHITECTURE.md` §10 and plan 05).

## 5. Mission plans

A mission plan is a named configuration baseline plus its defended-asset list,
geofences, policy, sensor and effector laydown, and the model baselines in force
(`gungnir-modelops`), with a validity period. Plans are created, validated, and
applied through `gungnir-config` and `gungnir-mission`; applying a plan is a
supervisor or commander decision and is audited.

## 6. Rehearsal

Rehearsal runs a plan against a scenario before the shift: replay a recorded session
or a test-track scenario through the whole pipeline with the new plan and watch what
the system would recommend and where the gaps are. `gungnir-replay` and the test-track
suite (plan 07) provide the inputs; the same desktop runs it in a replay session so
the operators rehearse on the screens they will use. Rehearsal outcomes feed the gap
register.

## 7. Battle rhythm

| Event | Content | System support |
|---|---|---|
| Shift handover | State of the picture, open alerts, pending approvals, degraded sensors, the plan in force | Handover summary (a report over the last shift's journal; an AI-assistant draft in plan 08) |
| Plan change | New defended-asset priorities, new laydown, new policy | Validated baseline; audited apply |
| Model promotion | A new filter or classifier configuration promoted after validation | `gungnir-modelops` promotion and rollback |
| Sensor maintenance windows | Coverage reduced deliberately | Mode changes; coverage shown |
| Reporting cycle | Situation reports to higher command | `gungnir-reporting` exports |

## 8. After-action review

Every session is journaled; the review replays it, recomputes what happened
(`gungnir-reporting` counts and metrics), reads the decisions and their timing, and
records lessons as gap entries, policy changes, or training points. The review is
also where the measures in `measures.md` are actually measured.

## 9. Thread

MT-09 in `mission-threads.md`: from a defended-asset list and a threat estimate to a
validated, rehearsed, applied plan.

## 10. Decision points

| Decision | Held by | System obligation |
|---|---|---|
| Set defended-asset priorities | Commander | Show consequences on coverage and allocation |
| Apply a plan | Supervisor or commander | Validation results; audit |
| Accept a coverage gap | Commander | Gap shown on the map and in the plan |
| Promote or roll back a model baseline | Analyst with supervisor concurrence | Validation evidence; rollback available |

## 11. What the system does today, and what it does not

| Function | Today | Not yet |
|---|---|---|
| Configuration baselines | Validation, file store, backend and node settings, resources | Defended-asset list, policy configuration, plan validity periods |
| Coverage analysis | Line-of-sight, coverage volumes, sensor coverage regions | Map rendering of coverage; gap detection as a function |
| Rehearsal | Replay of recorded sessions | Replay of test-track scenarios through the live pipeline with a chosen plan |
| Model governance | Registry with validation, promotion, rollback | Wiring to the tracking configuration in force |
| After-action review | Report generation and export | Review workflow and lessons capture |

## 12. Sources and confidence

| Area | Source type | Confidence |
|---|---|---|
| Defended-asset prioritization factors | Public air-defense planning doctrine | H for the factors; M for their weighting |
| Laydown and coverage practice | Public doctrine and trade literature | M |
| Battle rhythm | Public command-post practice | M |

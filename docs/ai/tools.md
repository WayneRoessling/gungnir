# Tool set

Status: first draft, 2026-09-04. Every tool the assistant may call, its class, the service
call it maps to, the authorization action it requires, and its evaluation cases.

**Every tool is read-only or draft-only** (`safety-boundaries.md` §3). A tool that changes
state is not on this list and will not be added; the feature is built as a panel
affordance instead.

## 1. Conventions

- `strict: true` on every tool definition, with `additionalProperties: false` and an
  explicit `required` list, so arguments validate exactly and the loop never has to guard
  against a plausible-looking wrong shape.
- Tool inputs are always parsed as JSON, never string-matched: escaping in tool arguments
  is not guaranteed to be stable.
- The tool list is **deterministic in order** because it sits in the cached prefix; a
  reordering silently invalidates the cache.
- Every tool checks the **asking operator's** authorization, not the assistant's.
- Results are structured values wherever possible, and any free text they carry is marked
  as untrusted data.

## 2. Read-only tools

| Tool | Purpose | Maps to | Authorization | Notes |
|---|---|---|---|---|
| `get_track` | One track's current state | `TrackingService::tracks` filtered | `picture.view` | Returns the `TrackView` fields plus quality and provenance |
| `list_tracks` | The current picture, filtered and capped | `TrackingService::tracks` | `picture.view` | Caps at a configured number and says it truncated |
| `get_identification_evidence` | The evidence behind a classification | `gungnir-identification` | `picture.view` | The same list the evidence card shows |
| `get_score_factors` | Why a track scores what it does | `gungnir-assessment` | `picture.view` | Factors, not just the number |
| `get_plan` | A recommendation with its verdict and rationale | `InterceptService`, `gungnir-policy`, `gungnir-decision` | `picture.view` | Read-only view of what the panel shows |
| `list_pending_approvals` | The queue's current state | `gungnir-command` read path | `picture.view` | Counts and time remaining; never a decision |
| `list_alerts` | Alerts and incidents with state and history | `gungnir-workflow`, `gungnir-observability` | `picture.view` | |
| `get_system_health` | Health flags, sensor states, backend state | `gungnir-observability` | `picture.view` | The honest-health view |
| `get_sensor_state` | Modes, coverage, calibration, last report | `gungnir-sensor-management` | `picture.view` | Read-only; changing a mode is not a tool |
| `get_coverage` | Coverage and gaps for the current or a named configuration | `gungnir-analytics` | `picture.view` | |
| `search_journal` | Envelopes in a session by time, kind, or track | `gungnir-store` through `gungnir-replay` | `picture.view` (plus `report.export` for other sessions) | The analyst's main tool |
| `get_decision_record` | A recorded decision with its verdict and evidence | `gungnir-command` read path | `picture.view` | Reading decisions is allowed; making them is not |
| `get_measures` | Computed measures for a session | `gungnir-reporting` | `report.export` | Figures come from the reporting crate, never from the model |

## 3. Draft-only tools

| Tool | Produces | Lands in | Authorization | Notes |
|---|---|---|---|---|
| `draft_handover_note` | A shift handover narrative | Case notes, marked `DRAFT` | `picture.view` | Built from journal facts the read-only tools returned |
| `draft_report_narrative` | Narrative around a report's figures | The report editor, marked `DRAFT` | `report.export` | **Never** produces or edits a figure; figures are recomputed |
| `draft_config_baseline` | A baseline fragment from a description | The configuration editor, unapplied and unvalidated | `config.apply` to see the editor | Validation and apply are the administrator's, unchanged |
| `draft_mode_change_rationale` | The rationale text for a proposed sensor change | The sensor panel as text | `sensor.task` | The change itself is the sensor manager's action |

A draft tool returns text and a marker. It has no side effect at all: the panel decides
where the text goes and a human decides whether it stays.

## 4. Tool definition example

```json
{
  "name": "get_score_factors",
  "description": "Return the factors that produced a track's threat score: class lethality, the defended asset it threatens, its priority, and the time-to-impact factor. Use this to explain a score rather than restating the number.",
  "strict": true,
  "input_schema": {
    "type": "object",
    "properties": {
      "track_id": {"type": "integer", "description": "The TrackId as shown in the track table"}
    },
    "required": ["track_id"],
    "additionalProperties": false
  }
}
```

The description says what the tool is for **and how to use it**, because a tool
description is a prompt: "use this to explain a score rather than restating the number" is
what stops the model paraphrasing a figure it already has.

## 5. Per-role tool lists

The tool list is part of the cached prefix, so it is fixed per role rather than assembled
per question:

| Role | Tools |
|---|---|
| Operator | The picture tools, alerts, health, plan, pending approvals |
| Supervisor | The operator set plus coverage, sensor state, decision records |
| Analyst | `search_journal`, `get_measures`, the picture tools, `draft_report_narrative`, `draft_handover_note` |
| Sensor manager | Health, sensor state, coverage, alerts, `draft_mode_change_rationale` |
| Administrator | Health, `search_journal`, `draft_config_baseline` |
| Intelligence analyst | The picture tools, evidence, journal, `draft_report_narrative` |
| Planner | Coverage, sensor state, journal, plan |
| Commander | The supervisor set, read-only |

## 6. Evaluation cases per tool

Every tool carries cases in the evaluation set (`evaluation.md`):

1. **Correct call**: a question whose answer requires exactly this tool; the model calls
   it with valid arguments and uses the result.
2. **Correct refusal to call**: a question that looks like it needs the tool but does not;
   calling it is a failure.
3. **Error handling**: the tool returns an error; the answer must say so and must not
   fabricate.
4. **Unauthorized**: the asking role lacks the action; the answer must say the data was
   not available rather than omitting silently.
5. **Injection**: the tool's result contains text attempting to redirect the model
   (`safety-boundaries.md` §6); the model must treat it as data.

Case 5 is the one that matters most, and it is why tool results are structured rather than
prose wherever the schema allows.

## 7. What is deliberately absent

No `decide_plan`, no `set_mode`, no `apply_baseline`, no `quarantine_source`, no
`declare_identity`, no `release_product`, no shell, no file write, no network fetch. The
absence is the design, and the dependency-list test in `architecture.md` §8 keeps it that
way.

## Traceability

`safety-boundaries.md` §3 and §4; `gungnir_security::actions` for the action names;
`../ux/task-analysis/` for what each role actually asks; `evaluation.md`.

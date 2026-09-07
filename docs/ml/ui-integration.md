# UI integration

Status: first draft, 2026-09-04. How model output reaches an operator, following the
designs in `../ux/`. The governing rule from plan 06's principle 7, extended from the
assistant to every learned component: **model output is labelled, sourced, and never has
authority.**

## 1. Rules

1. **A model never gets its own panel.** Its output appears inside the panel that already
   owns that information, as one more piece of evidence or one more factor. A "machine
   learning" panel would invite an operator to treat the model as a separate authority.
2. **Always labelled with the model and version.** `ml:classifier@1.2.0`, not "AI" and
   not an unattributed number.
3. **Always with its confidence**, and drawn so that low confidence looks low.
4. **Never auto-applied.** No model output changes a classification, a score, a plan, a
   quarantine, or a sensor mode by itself.
5. **Absence is visible.** When a model is off, failed, or out of domain, the panel says
   so where the output would have been. An operator must never mistake "the model is not
   running" for "the model found nothing".

## 2. ML-01 classification, in the evidence card

The evidence card (`../ux/wireframes/WF-04-track-detail-evidence.puml`) already lists
every piece of evidence with its source, kind, weight, and time. The model is one row:

```
Source          Kind              Weight   Time       Detail
A3 acoustic     signature class   +0.35    01:39:12   propeller, 2-blade class
R1 radar        kinematics        +0.30    01:40:02   300 m/s, 200 m AGL
ml:classifier@1.2.0  model        +0.22    01:40:05   hostile 0.78, unknown 0.19  [why]
operator        designation       +0.10    01:40:40   Mira: confirmed hostile
```

- The row is visually identical in weight to the others, because in the fusion it is.
- **"why"** opens the model's top contributing features, its calibration on the current
  domain, and its card. An operator asked to justify a hostile declaration has to be able
  to say what the model contributed and why.
- When the model is absent or out of domain, the row reads
  `ml:classifier@1.2.0 — not running (model absent)` or `— out of domain`, greyed, with
  no weight.
- The card shows what the classification would be **without** the model row, so the
  operator can see whether the model changed the answer. This is the single most useful
  affordance in the design and it costs one line.

## 3. ML-04 anomaly detection, in the alerts panel

Anomalies appear as incidents in the alerts panel
(`../ux/wireframes/WF-08-alerts-incidents.puml`), never as a separate stream:

```
Sev        Incident                                     Since      State  Raw  Source
▌ warning  Vessel 47: reporting off, loitering near      13:12:04   new    2    rules + ml:anomaly@0.3.0
           the outfall
```

- Where the rule-based detector (D-13) and the model both fire, the incident correlates
  into one and names both sources. Where only the model fires, the source column says so
  and the operator can weigh it accordingly.
- The incident summary names **the contributing feature**, not a bare score: "reporting
  off, loitering" rather than "anomaly score 0.87".
- Acknowledge, escalate, and close are the normal lifecycle. A model-sourced incident is
  not special and cannot be dismissed differently.
- The model never quarantines a source. If an operator wants a source stopped, that is a
  sensor-management action with its own authority and audit.

## 4. The status strip

The status strip (`../ux/information-architecture.md` §2) is where "health is reported,
never inferred" is enforced for every layout. Models join it:

- A chip when any enabled model is not running: `models: 1 of 2`, opening the health
  panel.
- Nothing when every enabled model is loaded and inside budget, because a chip that is
  always present is a chip nobody reads.

## 5. Shadow mode is invisible to operators

While a model is in shadow, its output is journalled and read by the analyst in replay.
It appears in no live panel at all. Showing a shadow model's output to an operator would
make it consumed in the only way that matters, which is by a person.

## 6. The analyst's view

The analyst is the one role that sees models directly
(`../ux/task-analysis/analyst.md` T-an-3): the model registry with promotion state,
validation evidence, the card, drift signals, and the shadow-mode comparison. Promotion
is a dialog that shows the evidence and refuses without it, which is the existing
`gungnir-modelops` semantics rather than a new rule.

## 7. What the design deliberately refuses

- A confidence bar with no number.
- Any phrasing that attributes agency to the model: it "suggests", it does not "think",
  "believe", or "decide".
- A global "AI on" toggle. Models are enabled individually in the configuration baseline,
  audited, because "the AI" is not a thing that can be reasoned about but a specific
  model with a specific domain is.

## Traceability

Principles and panels: `../ux/README.md`, `../ux/information-architecture.md`;
wireframes WF-04, WF-08, WF-09; use cases `use-cases.md`; governance `mlops.md`.

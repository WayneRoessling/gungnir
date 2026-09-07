# Concept of operations

Status: first draft, 2026-09-04. What the assistant does for each role, what it never
does, and what an exchange looks like.

## 1. What it is

A read-only, draft-only assistant inside the operator interface. It answers questions
about the live or recorded picture, explains scores, plans, and alerts, and drafts
products a human then edits and owns. It holds no authority: no path in the system lets
it change a plan, a decision, a quarantine, a configuration baseline, or a sensor mode.

It is not a co-pilot for engagement decisions. Under saturation the bottleneck is the
authority's attention (MOE-04), and the assistant's job is to reduce the work of
understanding, not to pre-decide.

## 2. Per role

| Role | It helps with | It never |
|---|---|---|
| Operator (P-01) | "What is track 42 and why is it scored high?", "Which alerts are new since the raid started?", "Summarize the last five minutes" | Accepts, overrides, or rejects a plan; changes a sensor mode; declares an identity |
| Supervisor (P-02) | Queue triage summaries, side-by-side explanation of alternatives, drafting a shift handover | Decides anything in the queue; sets weapons control status |
| Analyst (P-03) | Questions over a replayed session, after-action narrative drafts, describing patterns across a shift | Alters the journal, or any figure in a report; figures are recomputed by `gungnir-reporting` |
| Sensor manager (P-04) | Coverage questions, drafting a mode-change proposal with its rationale | Applies a mode change or a tasking |
| Administrator (P-05) | Drafting a configuration baseline from a description, explaining a validation failure | Applies a baseline |
| Intelligence analyst (P-06) | Summarizing evidence across a track's lineage, drafting a product narrative | Declares an identity; releases a product |
| Planner (P-07) | Comparing two laydowns in words, drafting the rehearsal record | Submits or applies a plan |
| Commander (P-08) | "What changed in the last hour?", summarizing the queue's state and accepted gaps | Delegates, accepts a gap, or approves a plan |

The pattern is the same in every row: **it drafts and explains; a human decides and
signs.**

## 3. What an exchange looks like

From vignette VG-01 at 01:41, mid-raid (`../ux/wireframes/WF-19-assistant.puml`):

> **Operator:** why is T-042 scored 87?
>
> **Assistant** · sources: snapshot 01:41:20 · tools: `get_track`, `get_score_factors`
> · model `claude-opus-5`:
> T-042 is confirmed hostile (0.91) on acoustic signature and radar kinematics,
> predicted to reach OPS (priority 1) in 3:05. The score is class lethality 0.9 times
> asset priority 1 times a time factor of 0.8. It would drop to about 22 if the class
> were a decoy.

Three things make that answer acceptable rather than merely fluent: the snapshot time,
so the operator knows how fresh it is; the tools it called, so the operator knows where
the numbers came from; and a link into the panel that holds the real figures, so the
answer is a route to the evidence rather than a substitute for it.

## 4. Drafts

A draft is labelled `DRAFT, not a record` and goes only to a place a human edits:

- A shift handover note into the case notes.
- An after-action narrative into a report, **around** the figures, never replacing them.
- A configuration baseline into the editor, unapplied and unvalidated until the
  administrator validates it.
- A mode-change proposal into the sensor panel as text the sensor manager retypes or
  accepts as their own.

No draft becomes an action by being approved in the assistant panel. The human moves it
into the panel that owns it, and that panel's normal authority and audit apply.

## 5. Disconnected and degraded

The assistant is a convenience, and it degrades honestly:

- **Disconnected desktop or air-gapped:** a local open-weight model with a reduced tool
  set and journal-only scope. The panel says which provider answered.
- **No provider reachable:** the panel says so and offers nothing. It does not fall back
  to a cached answer or a canned response.
- **Under saturation:** the assistant is the first thing an operator ignores, so it must
  never occupy the queue's screen space or interrupt (plan 06 principle: nothing pops
  over the queue).

## 6. Why the loop runs in our process

Anthropic offers a managed agent runtime that would host the loop and a tool sandbox.
This design deliberately does not use it, for two reasons that are specific to this
product:

1. **Audit.** Every exchange must be in `gungnir-security`'s audit log with the model,
   the tools called, the data they returned, and the mission time. Owning the loop makes
   that a local write rather than a reconciliation with someone else's event stream.
2. **Egress.** The on-prem and disconnected profiles restrict what may leave the host.
   A hosted loop would decide what to send; here the egress policy decides, in our code,
   before anything leaves (`security.md`).

The cost is that we write the loop. It is a small loop, and both reasons are
requirements rather than preferences.

## Traceability

Roles and panels: `../ux/personas.md`, `../ux/wireframes/WF-19-assistant.puml`;
capability CAP-4.7; boundary CAP-4.3; measure MOP-35; gap GAP-044.

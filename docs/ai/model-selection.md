# Model selection

Status: first draft, 2026-09-04. Which models, why, and how to revisit. Model facts are
current as of 2026-09-04 and should be re-checked when the provider is written.

## 1. Connected profiles

**Default: `claude-opus-5`.**

| Why | Detail |
|---|---|
| Capability at the task | The hard questions are explanation and synthesis over a live picture with tool use, which is where the strongest generally available model earns its cost |
| Context | A one-million-token window means a journal search or a long session summary does not need to be chunked, which removes a whole class of retrieval bugs |
| Thinking and effort | Adaptive thinking with a per-task effort setting gives one model that can be cheap for triage and thorough for explanation, instead of two models and two caches |
| Refusal handling | A declined request returns a category rather than a failure, and server-side fallbacks turn a decline into a degraded answer rather than nothing |

**Effort per task**, because effort is the first cost lever after caching:

| Task | Effort | Reason |
|---|---|---|
| Alert and queue triage summaries | `low` | Short, factual, high volume |
| Question answering over the snapshot | `medium` | The common case |
| Explaining a score, a plan, or a disagreement | `high` | The answer must be right and must reason over several sources |
| After-action narrative over a whole session | `high` | Long context, synthesis |

`max` is not used. If a question needs it, the answer probably needs a person.

**A cheaper model for bulk drafting.** `claude-haiku-4-5` is a candidate for high-volume,
low-judgment drafting, but only after the evaluation set shows quality holds on those
specific tasks. Two cautions kept in view: caches are model-scoped, so a second model
forfeits the cached prefix, and a cheaper model that needs more turns is not cheaper.
Measure cost per completed task, not per request.

**Not `claude-fable-5-1`.** It is the more capable model, at twice the price, and its
advantages are in long-horizon autonomous work. This assistant does short, tool-grounded
turns for a person who is waiting. If evaluation later shows explanation quality is the
binding constraint, it is the first thing to try, and the provider trait makes that a
configuration change.

## 2. Disconnected profiles

An open-weight instruction-tuned model served locally, sized to the host
(`provider-configuration.md` §3). The selection criteria, in order:

1. **Instruction following with tools.** A model that cannot reliably produce a valid tool
   call is not usable here regardless of its prose quality.
2. **Licence.** The weights must be redistributable in the product's context, which is a
   legal question under the US jurisdiction decision, not only a technical one.
3. **Size against the host**, measured against the latency budgets rather than assumed.
4. **Quality on the evaluation set**, scored with the same harness as the connected
   provider so the comparison is real.

No specific model is named here on purpose: the open-weight field moves faster than this
document will be revised, and the criteria outlast any name. The choice is recorded in the
deployment's configuration with its version and hash.

## 3. How the choice is revisited

- The evaluation set (`evaluation.md`) is the instrument. A model change is a measured
  change, not an upgrade.
- Re-run when: a new model is released, the assistant's task mix changes, cost per session
  moves materially, or the local provider's host changes.
- Model identifiers are configuration, not code. Changing one is a baseline change and is
  audited.
- Prompts are tuned per model. A system prompt written for one model is not assumed to be
  right for another, and the evaluation catches the difference.

## 4. What is not delegated to model choice

No model choice changes the safety boundary. A more capable model does not earn more
authority, a cheaper one does not lose the audit requirement, and a local one does not get
a relaxed egress rule. The boundary is architectural (`safety-boundaries.md`), and the
model is an implementation detail behind a trait.

## Traceability

`provider-configuration.md` for the request shapes; `evaluation.md` for the instrument;
`../business/financial-model.md` for why per-session cost is tracked.

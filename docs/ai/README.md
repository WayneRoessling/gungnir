# AI agent integration

Deliverables of [plan 08](../plans/08-ai-agent-integration.md): an assistant inside the
operator interface that answers questions, explains the picture, and drafts products.

**The standing rule: the agent reads and drafts; it has no authority to act.** It is
enforced twice, so that neither a prompt nor a future feature request can erode it: no
tool exists that changes state, and the crate cannot reach the code that would.

Status: first draft 2026-09-04. Nothing is built. `gungnir-agent` does not exist, no
evaluation case has been run, and the security reviewer has not seen `security.md`.

| File | Content |
|---|---|
| [`concept-of-operations.md`](concept-of-operations.md) | What it does per role, what it never does, what an exchange looks like, and why the loop runs in our process |
| [`safety-boundaries.md`](safety-boundaries.md) | The authority model, the two tool classes, authorization, audit, untrusted text, egress, failure behaviour. **Owner signs this off before any code** |
| [`architecture.md`](architecture.md) | The `gungnir-agent` crate: the `LlmProvider` trait, the loop, context assembly and caching, the three providers, failure behaviour, testing |
| [`tools.md`](tools.md) | Seventeen tools with their class, service call, authorization action, and evaluation cases; the per-role lists; what is deliberately absent |
| [`provider-configuration.md`](provider-configuration.md) | Per-profile settings and the exact request shapes: headers, adaptive thinking, effort, caching, streaming, stop reasons, refusal fallbacks, local model classes, credentials |
| [`model-selection.md`](model-selection.md) | Which models and why, effort per task, how the choice is revisited, and what model choice never changes |
| [`ui-integration.md`](ui-integration.md) | The assistant panel, inline affordances, streaming, per-role scope, and what the design refuses |
| [`services-integration.md`](services-integration.md) | The node-side assistant, after-action drafting in batch, why peer systems may not ask, caching, quotas |
| [`evaluation.md`](evaluation.md) | What is measured, where cases come from, the seven injection cases, proposed thresholds, and how it runs in CI |
| [`security.md`](security.md) | Eight threats with controls, prompt injection and egress in detail, what is logged, and the reviewer's checklist |

## Decisions recorded here

Three of plan 08's open questions are answered in these documents rather than left open,
each with its reasoning:

| Question | Answer | Where |
|---|---|---|
| Does the loop run in Anthropic's managed runtime or ours? | **Ours.** Every exchange must be in our audit log, and the egress policy must be applied by our code before anything leaves the host | `concept-of-operations.md` §6 |
| Where does after-action drafting run? | **On the node, in batch.** Nobody is waiting, batch costs half, the journal is already there, and it cannot slow a desktop mid-raid | `services-integration.md` §2 |
| Which local model size classes? | 7 to 14 billion parameters quantized on a desktop with a GPU; 3 to 8 billion on a node CPU, with the consequences stated to the operator. **Engineering assumptions to be measured**, not claims | `provider-configuration.md` §3 |

## What this plan is careful about

- **Over-trust is the likely harm, not a rogue agent.** The controls that matter are the
  provenance line, the links into the panels that hold the real figures, the absence of a
  confidence score on prose, and a usability task that measures whether operators believe
  it too readily.
- **Injection is mitigated, not solved.** The reason the residual risk is acceptable is
  that a successful injection produces a wrong answer to a person who can check it, not an
  action.
- **The API facts here have a date on them.** Model identifiers, parameters, and beta
  headers were current on 2026-09-04 and should be re-checked against the current
  documentation when the provider is written.

## Engineering items

GAP-044 covers the crate, the providers, the tools, the panel, and the evaluation harness,
and it depends on D-14 (the egress policy, resolved) and shares an HTTP client sign-off
with GAP-041. MOP-35's thresholds are proposed in `evaluation.md` §5 for the owner.

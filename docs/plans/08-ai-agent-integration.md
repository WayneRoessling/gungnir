# Plan 08: AI agent (LLM-based) integration into the UI and services

## Purpose

Give operators, supervisors, and analysts an assistant that answers questions about
the live and recorded picture, explains recommendations and alerts, drafts reports
and configuration, and triages alerts, in every deployment profile, with no authority
to act. The agent is a reader and a drafter; every action it proposes goes through
the same policy verdict and recorded human decision as anything else.

## Scope

In scope: the concept of operations, safety boundaries, a `gungnir-agent` crate with
one provider trait and two providers (Claude API when connected, open-weight local
models when not), a tool set over the existing read-only services and draft-only
proposal paths, UI surfaces per role, a node-side agent for API consumers,
evaluation, cost control, and security including prompt-injection defenses for
untrusted sensor and message text.

Out of scope: any path by which the agent changes a plan, a decision, a quarantine,
a configuration baseline, or a sensor mode without a human decision; autonomous
operation; training or fine-tuning models (plan 09 covers learned models).

## Decision applied

Cloud LLM when connected, local models when not, behind one provider trait
(`docs/plans/README.md`). The Claude API is used in the cloud and on-prem-with-egress
profiles; open-weight instruction-tuned models served locally (llama.cpp or vLLM
class servers) are used in disconnected and air-gapped profiles. The profile's
configuration names the provider; the agent code does not know which is active.

## Inputs

- `docs/ux/` (plan 06) for where the assistant appears per role.
- `docs/mission/` (plan 02) for the questions operators actually ask.
- `gungnir-api` (the handler surface the agent's tools map onto), `gungnir-model`
  (what the agent can see), `gungnir-command` (how proposals become decisions),
  `gungnir-security` (who may ask what, and the audit log), `gungnir-store`
  (replay context), `gungnir-observability` (alerts to triage).
- The Claude API documentation for the Messages API, tool use, structured outputs,
  prompt caching, effort and adaptive thinking, batches, and refusal handling. Rust
  has no official Anthropic SDK, so the provider speaks the HTTP API directly.

## Deliverables and target location

All under `docs/ai/`:

| File | Content |
|---|---|
| `README.md` | Index and the standing rule: the agent has no authority |
| `concept-of-operations.md` | What the assistant does per role, what it never does, example interactions from the mission vignettes, how outputs are labelled and sourced |
| `safety-boundaries.md` | The authority model: read-only tools, draft-only proposals, mandatory human decision, audit of every exchange, data-egress rules per profile, refusal handling, what happens when the provider is unavailable |
| `architecture.md` | The `gungnir-agent` crate: `LlmProvider` trait, `AnthropicProvider` and `LocalProvider`, the tool registry, context assembly, the agent loop, provenance and audit hooks, cost and rate controls |
| `tools.md` | The tool set with JSON schemas: each tool's name, purpose, the service call it maps to, whether it is read-only or draft-only, authorization action required, and its evaluation cases |
| `ui-integration.md` | Assistant panel per role (`gungnir-workflow` layouts), inline "why" affordances on the plan, alert, and risk panels, streaming display, provenance labels, the disconnected-profile experience |
| `services-integration.md` | Node-side agent for API consumers (peer systems asking questions of the node), batching for report generation, caching, quotas |
| `provider-configuration.md` | Per-profile provider settings: model identifiers, effort, thinking, caching, fallbacks, endpoints, credentials handling, local model requirements and hardware |
| `evaluation.md` | The evaluation set built from vignettes and journals: factual questions with known answers, explanation quality, refusal correctness, prompt-injection resistance, latency and cost; how it runs in CI |
| `security.md` | Threat model: prompt injection through sensor and message text, data exfiltration, model supply chain for local weights, credential handling, logging of prompts and responses, classification handling |
| `model-selection.md` | Which models, why, and how to revisit: Claude Opus 5 as the connected default with adaptive thinking and effort tuned per task, a cheaper current-generation model for bulk drafting where measured quality holds, open-weight local models by size class and the hardware each profile has |

## Concept of operations (summary)

| Role | The assistant helps with | Never |
|---|---|---|
| Operator | "What is track 42 and why is it scored high?", "Which alerts are new since the last raid?", "Summarize the last five minutes" | Accept, override, or reject a plan; change a sensor mode |
| Supervisor | Approval-queue triage summaries, side-by-side explanation of alternatives, drafting shift handover notes | Decide on the queue |
| Analyst | Questions over a replayed session, after-action report drafting from the journal, pattern descriptions | Alter the journal or a report's figures (figures are recomputed by `gungnir-reporting`) |
| Sensor manager | Coverage questions, draft mode-change proposals with rationale | Apply the change |
| Administrator | Draft configuration baselines from a description, explain validation failures | Apply a baseline |

Every answer carries provenance: the model and version, the tools it called and the
data those returned, and the mission time of the snapshot it saw. Every exchange is
written to the `gungnir-security` audit log.

## Architecture (summary)

- **Crate `gungnir-agent`** (productization layer; depends on `gungnir-model`,
  `gungnir-security`, `gungnir-eventing`, and the read surfaces it wraps). New stack
  additions (an HTTP client, JSON is already present) are recorded in
  `docs/agentic-coding-standards.md` §2.9 when they land.
- **`LlmProvider` trait:** `complete(request) -> response` with streaming, tool-call
  and tool-result turns, structured outputs, a token and cost report per call, and a
  typed `Unavailable` error. Providers: `AnthropicProvider` over the Messages API
  (Claude Opus 5 by default, adaptive thinking, effort per task, prompt caching of the
  stable system prompt and tool definitions, strict tool schemas, server-side
  fallbacks and explicit refusal handling, streaming for long outputs, batches for
  report generation); `LocalProvider` over a local inference server's HTTP API for
  open-weight models, with the same trait surface and reduced feature flags.
- **Agent loop:** a manual tool-use loop owned by Gungnir, not an SDK helper, so every
  turn is logged, every tool call is authorized against the caller's role, parallel
  tool calls are executed and returned together, and a turn budget and cost cap end
  the loop. No prefill; no forced tool choice; JSON tool inputs are parsed, never
  string-matched.
- **Tools:** read-only wrappers over the snapshot, track detail, risk scores and
  rationale (`gungnir-decision::rationale_for`), alerts, coverage, geofences,
  line-of-sight (`gungnir-analytics`), replay and reports; draft-only proposals that
  create a `PendingApproval` in `gungnir-command`, a candidate baseline in
  `gungnir-config`, or an annotation in `gungnir-workflow`. No tool has a side effect
  beyond creating a draft.
- **Context assembly:** a frozen system prompt per role, then the role's tool list,
  then volatile context (mission time, snapshot summary, the question) after the last
  cache breakpoint. Untrusted text (sensor free-text fields, peer-system messages,
  annotations) is quoted as data and never as instructions.
- **Cost and rate controls:** per-role quotas, per-session cost cap, cached prefixes,
  a cheaper model for bulk drafting where the evaluation shows quality holds, batches
  for after-action reports.

## Provider configuration (summary)

| Profile | Provider | Notes |
|---|---|---|
| Cloud connected | Claude API | Opus 5 default; effort tuned per tool (low for triage summaries, high for explanations); server-side fallbacks on; data-egress rules from `security.md` |
| On-prem connected with egress | Claude API | As cloud, subject to the deployment's egress policy; local fallback configured |
| On-prem without egress, disconnected desktop, air-gapped | Local model | Open-weight instruction-tuned model sized to the host (desktop GPU or node CPU); reduced feature set; the assistant states its provider in every answer |

Credentials never enter the sandbox of any tool and never appear in prompts; they are
held by the provider configuration and the operating system's secret store.

## Method

1. **Concept and boundaries first.** Write the concept of operations and the safety
   boundaries; owner sign-off before any code.
2. **Tools.** Specify the tool set with schemas and authorization actions; every
   tool gets evaluation cases.
3. **Crate.** Scaffold `gungnir-agent`: the trait, the loop, the audit hook, a fake
   provider for tests; then `AnthropicProvider`, then `LocalProvider`.
4. **UI.** The assistant panel and inline affordances per plan 06; provenance
   labels; streaming.
5. **Evaluation.** Build the set from vignettes and journals; run it in CI against
   the fake provider for regressions and on demand against real providers; record
   cost.
6. **Security review** (human-owned): prompt-injection tests, egress rules, logging,
   model supply chain for local weights.
7. **Node-side agent** once the API transport exists.

## Roles

- Owner: concept of operations, safety boundaries, provider and egress policy,
  cost caps.
- Agent engineering: crate, providers, tools, UI panel, evaluation harness.
- Security reviewer (human-owned): threat model, red-team pass, sign-off.

## Dependencies

Plan 06 for surfaces; `gungnir-api` transport for the node-side agent
(`ARCHITECTURE.md` §10); stack sign-off for the HTTP client. The desktop-side agent
against the embedded services can proceed before the transport.

## Effort and sequencing

15 to 25 agent-assisted days for the documents, the crate with both providers, the
first tool set, the panel, and the evaluation set; 4 weeks elapsed after plan 06.

## Acceptance criteria

- No code path lets the agent change a plan, decision, quarantine, baseline, or
  sensor mode; the draft-only tools create pending items that a human decides.
- Every exchange is in the audit log with model, tools, data, and mission time.
- The same question gets an answer in all three profiles, with the provider named.
- The evaluation set runs in CI and reports factual accuracy, refusal correctness,
  and injection resistance; regressions fail the build.
- Cost per session is capped and reported.

## Risks

- Over-trust in fluent answers; mitigate with provenance labels, the evaluation set,
  and the no-authority rule.
- Prompt injection through sensor or peer text; mitigate by quoting untrusted text
  as data, strict tool schemas, and red-team tests.
- Local models under-perform; mitigate by measuring against the same evaluation set
  and stating the provider in every answer.
- Data egress in connected profiles; mitigate with the per-profile egress policy
  and a classification check before any prompt leaves the host.

## Open questions

- Which local model size classes the desktop GPU and the node CPU can serve within
  the latency budgets.
- Whether after-action report drafting runs on the node in batch or on the desktop.
- Whether Anthropic's hosted agent runtime is acceptable for the cloud profile's
  report drafting, or whether every loop must run in Gungnir's own process for
  audit reasons; the plan assumes the latter.

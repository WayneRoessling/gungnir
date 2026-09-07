# Provider configuration

Status: first draft, 2026-09-04. Per-profile provider settings and the exact request
shapes. API facts are current as of 2026-09-04; they move, and the provider code should be
checked against the current documentation when it is written.

## 1. Per profile

| Profile | Provider | Model | Notes |
|---|---|---|---|
| Cloud connected | Claude API | `claude-opus-5` | Live picture may be sent (D-14); effort tuned per task |
| On-prem connected with egress | Claude API | `claude-opus-5` | Allow-listed derived text only; local fallback configured |
| On-prem without egress | Local | Open-weight instruction-tuned | Nothing leaves the host |
| Disconnected desktop | Local | Open-weight, sized to the host | Journal-only scope, reduced tool set |
| Air-gapped | Local | As above | Same |

The profile's configuration names the provider. The agent code does not know which is
active, and the panel states the provider in every answer.

## 2. Claude API

**Endpoint** `POST https://api.anthropic.com/v1/messages`.

**Headers**

| Header | Value |
|---|---|
| `content-type` | `application/json` |
| `x-api-key` | The API key, from the operating system's secret store |
| `anthropic-version` | `2023-06-01` |
| `anthropic-beta` | Only when a beta feature is used (see §4) |

**A request, with the parameters this design uses**

```json
{
  "model": "claude-opus-5",
  "max_tokens": 16000,
  "stream": true,
  "thinking": {"type": "adaptive", "display": "summarized"},
  "output_config": {"effort": "high"},
  "tools": [
    {"name": "get_score_factors", "description": "...", "strict": true,
     "input_schema": {"type": "object", "properties": {"track_id": {"type": "integer"}},
                      "required": ["track_id"], "additionalProperties": false}}
  ],
  "system": [
    {"type": "text", "text": "<frozen role prompt>", "cache_control": {"type": "ephemeral"}}
  ],
  "messages": [
    {"role": "user", "content": "<snapshot summary, mission time, question>"}
  ]
}
```

Notes that matter, each of which is a real constraint rather than style:

- **Thinking is on by default on `claude-opus-5`**; `{"type": "adaptive"}` is equivalent
  to omitting it. `budget_tokens` is removed and returns 400. `temperature`, `top_p`, and
  `top_k` are removed and return 400.
- **`display: "summarized"`** is set deliberately: the default omits the reasoning, which
  in a streaming panel looks like a long unexplained pause.
- **Effort** lives inside `output_config`, not at the top level. Default is `high`.
- **`max_tokens`**: about 16,000 for non-streaming, more for streaming. Do not lowball it;
  a truncated answer costs a retry.
- **Caching**: the breakpoint goes on the last block of the stable prefix; the render order
  is tools, then system, then messages. Verify with
  `usage.cache_read_input_tokens`.
- **No assistant prefill.** It returns 400 on this model. Response shape is controlled with
  the system prompt or structured outputs (`output_config.format`), not by prefilling.
- **Mid-conversation operator instructions** go in `messages` as a `system` role entry
  rather than by editing the top-level `system`, which would invalidate the cache and is
  also the injection-safe channel.

**Reading the response**

Check `stop_reason` before touching `content`:

| `stop_reason` | Meaning | What the loop does |
|---|---|---|
| `end_turn` | Finished | Display |
| `tool_use` | Wants tools | Execute, return all results in one user message, loop |
| `max_tokens` | Truncated | Report truncation to the operator; do not silently continue |
| `refusal` | The model declined | `stop_details` carries the category and explanation; report it; never retry reworded |

`stop_details` is populated **only** for `refusal` and is null otherwise, so it is always
guarded.

**Refusal fallbacks.** For `claude-opus-5` the server-side fallback parameter is available
and is enabled by default here, so a policy decline is re-run on a fallback model inside
the same call rather than surfacing as a dead end mid-raid. It is not available on the
Batches API, so the batch path in `services-integration.md` handles refusal directly.

**Streaming** is Server-Sent Events: `message_start`, `content_block_start`,
`content_block_delta`, `content_block_stop`, `message_delta` (which carries the final
`stop_reason` and usage), `message_stop`. The panel renders text deltas; the provider
accumulates blocks and returns the finished turn.

**Errors**: 429 and 5xx are retried with backoff a bounded number of times; 400 and 404
are not. A rate-limit retry is bounded because an operator waiting mid-raid needs an
answer or an honest failure, not a long silent retry loop.

## 3. Local provider

An open-weight instruction-tuned model served on loopback by a local inference server.
The provider speaks that server's HTTP interface directly; it does **not** pretend to be
the Claude API, and no compatibility shim is used, so the differences stay visible where
they belong, in the provider.

Size classes, as engineering assumptions to be measured rather than claims:

| Host | Class | Expectation |
|---|---|---|
| Desktop with a discrete GPU | 7 to 14 billion parameters, quantized | A few seconds to first token; usable for questions and short drafts |
| Node, CPU only | 3 to 8 billion parameters, quantized | Tens of seconds; usable for offline drafting, not for an operator waiting mid-raid |

Consequences that are stated to the operator rather than hidden: a reduced tool set, no
long-context journal search, and shorter drafts. The panel names the provider and the
model in every answer so the operator can calibrate.

Local model weights are a supply-chain concern and are treated like model artefacts in
plan 09: pinned version, hash verified before load, and no automatic download
(`security.md`).

## 4. Betas in use

Only where a feature is genuinely needed, because each one is a header and a coupling to a
dated feature:

| Feature | Beta | Why |
|---|---|---|
| Server-side refusal fallbacks | `server-side-fallback-2026-07-01` with `fallbacks: "default"` | A decline mid-raid should degrade to a fallback model, not to nothing |

Everything else this design uses is generally available: adaptive thinking, effort, strict
tools, caching, streaming, mid-conversation system messages, and batches.

## 5. Cost and rate control

- Per-role quota and a per-session cost cap, enforced in the loop, reported to the
  operator when reached.
- The cached prefix is the main lever: a stable system prompt and tool list mean the
  per-question cost is the volatile tail plus the answer.
- A cheaper current-generation model (`claude-haiku-4-5`) for bulk drafting **only where
  the evaluation shows quality holds**, never as an unmeasured default. Caches are
  model-scoped, so a second model has its own cache.
- Batch processing at reduced cost for after-action drafting
  (`services-integration.md`).
- `usage` is recorded per exchange in the audit entry, so cost is attributable to a role
  and a session rather than being a monthly surprise.

## 6. Credentials

The API key lives in the operating system's secret store, is read by the provider at
construction, and **never appears in a prompt, a tool argument, a log line, an audit
entry, or an error message**. Rotation is a configuration action. In profiles with no
egress there is no key at all.

## Traceability

`architecture.md` for the loop; `safety-boundaries.md` §7 for egress; `security.md` for
the threat model; `model-selection.md` for the choice of model.

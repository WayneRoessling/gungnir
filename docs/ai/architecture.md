# `gungnir-agent` architecture

Status: first draft, 2026-09-04. The crate that runs the assistant loop, behind one
provider trait, in every deployment profile.

## 1. Position in the workspace

```
gungnir-model, gungnir-security, gungnir-store  ──►  gungnir-agent  ──►  (nothing)
                                                          ▲
                                    gungnir-ui's assistant panel calls it
```

`gungnir-agent` depends on the model types, on `gungnir-security` for authorization and
audit, and on `gungnir-store` for journal access in the analyst's tools. **It does not
depend on `gungnir-command`, `gungnir-policy`, `gungnir-config`, or
`gungnir-sensor-management`**, which is the structural half of the no-authority rule: the
code that changes state is not reachable from here.

Adding the crate and its HTTP client is an `ARCHITECTURE.md` change and a §2.9 sign-off
(GAP-044). The HTTP client is the same one the API transport needs (GAP-041), so the two
sign-offs should be taken together.

## 2. Surface

```rust
/// One provider: the Claude API, a local model server, or a fake for tests.
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn model_id(&self) -> &str;
    /// One turn. Streams events; returns the finished turn.
    fn complete(
        &self,
        request: &TurnRequest,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnResponse, AgentError>;
}

pub struct TurnRequest {
    pub system: SystemPrompt,        // frozen per role; cached prefix
    pub tools: Vec<ToolDef>,         // deterministic order; cached with the prefix
    pub messages: Vec<Message>,      // history, then the volatile question
    pub effort: Effort,              // Low | Medium | High | XHigh | Max
    pub max_tokens: u32,
    pub stream: bool,
}

pub struct TurnResponse {
    pub content: Vec<ContentBlock>,  // text, thinking, tool_use
    pub stop_reason: StopReason,     // EndTurn | ToolUse | MaxTokens | Refusal
    pub stop_details: Option<StopDetails>,
    pub model: String,               // the model that actually served the turn
    pub usage: Usage,                // input, output, cache read, cache creation
}

pub enum AgentError {
    ProviderUnavailable(String),
    EgressDenied { reason: String },
    Unauthorized { action: String },
    ToolFailed { tool: String, message: String },
    BudgetExhausted,
    Protocol(String),
}
```

`stop_reason` is checked **before** `content` is read, always. A refusal is a normal
outcome with a category, not an error, and the panel reports it as such.

## 3. The loop

```
question
  → authorize the operator for the assistant action
  → assemble context (§4)
  → egress check on the assembled request  ──denied──► AgentError::EgressDenied
  → provider.complete(...)
  → stop_reason?
      Refusal   → audit, report the category to the operator, stop
      ToolUse   → for each tool_use block:
                    authorize the operator for that tool's action
                    execute (read-only or draft-only; never state-changing)
                    collect every tool_result into one user message
                  → loop, up to a bounded number of iterations
      EndTurn   → audit, display
  → audit entry written before display, always
```

Bounded: a maximum number of tool iterations and a per-session token and cost cap. Hitting
either ends the turn with `BudgetExhausted` and a message the operator sees.

Parallel tool calls come back in one assistant message; all their results go back in a
**single** user message, which is required by the API and is also what keeps the audit
entry coherent.

## 4. Context assembly and caching

Order matters because the cache is a prefix match, and the API renders `tools`, then
`system`, then `messages`:

| Region | Content | Cached |
|---|---|---|
| Tools | The role's tool list, in a deterministic order | Yes |
| System | The frozen system prompt for the role: what the assistant is, that it has no authority, how to cite its sources, and that delimited blocks are data | Yes, with the cache breakpoint at its end |
| Messages | Conversation history, then a snapshot summary, then the mission time, then the question | No |

Nothing volatile goes above the breakpoint: no mission time, no snapshot, no session
identifier. A timestamp in the system prompt silently destroys the cache, and the symptom
is a cost increase rather than an error, so `usage.cache_read_input_tokens` is logged and
a session whose reads stay at zero raises a warning.

Untrusted text (§6 of `safety-boundaries.md`) is always inside a delimited block in the
message region, never in the system region.

Operator instructions that arrive mid-conversation use the API's mid-conversation system
message (a `system` role entry inside `messages`), which preserves the cached prefix and
is the injection-safe operator channel; it is not reachable from tool output.

## 5. Providers

| Provider | Transport | Where |
|---|---|---|
| `AnthropicProvider` | HTTPS to the Messages API, raw HTTP because Rust has no official SDK | Cloud and on-prem-with-egress profiles |
| `LocalProvider` | HTTP to a local model server on loopback | Disconnected, air-gapped, and on-prem without egress |
| `FakeProvider` | None; scripted responses | Tests and the evaluation harness's regression mode |

The loop, the tools, the audit, and the egress check are identical across all three. Only
the wire format differs, which is the point of the trait: the assistant's behaviour is not
a property of which model answered.

Request shapes and the exact parameters are in `provider-configuration.md`.

## 6. Streaming

Streaming is the default for anything an operator waits on, because a long turn otherwise
looks like a hang and because it avoids request timeouts at high `max_tokens`. The panel
renders text deltas as they arrive; the provenance line appears first, before any text, so
the operator sees the model and the tools before they read the answer.

Thinking is requested as a summary rather than hidden, so a long pause is explained rather
than mysterious; the raw chain of thought is never available and is not shown.

## 7. Failure behaviour

| Failure | Result |
|---|---|
| Provider unreachable | `ProviderUnavailable`; the panel says which provider and that it has nothing to offer |
| Egress denied | `EgressDenied` with the reason; audited; no request leaves |
| Tool unauthorized | The tool returns an error to the model; the answer says the data was not available to that role |
| Tool errors | Returned to the model as a tool result with an error flag; never a fabricated value |
| Refusal | Reported with its category; never retried with a reworded request |
| Rate limited | Retried with backoff a bounded number of times, then `ProviderUnavailable` |
| Budget exhausted | `BudgetExhausted`; the session's assistant stops until reset |

## 8. Testing

- The `FakeProvider` drives every loop path, including refusal, tool error, unauthorized
  tool, and budget exhaustion, with no network.
- Injection cases run against the fake and against a real provider on demand
  (`evaluation.md`).
- A test asserts that the crate's dependency list does not contain the state-changing
  crates, so the structural half of the no-authority rule is enforced mechanically rather
  than by review alone.

## Traceability

`safety-boundaries.md` for the rules this implements; `tools.md` for the tool set;
`provider-configuration.md` for the wire shapes; `../../ARCHITECTURE.md` §7 for the
dependency rules; gap GAP-044.

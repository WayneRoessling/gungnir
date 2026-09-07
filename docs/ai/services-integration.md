# Services integration

Status: first draft, 2026-09-04. The node-side assistant: batch drafting, caching, quotas,
and what peer systems may and may not ask.

## 1. Node-side versus desktop-side

| | Desktop | Node |
|---|---|---|
| Who asks | The operator in front of it | A scheduled job, or an API caller |
| What it answers | Questions about the live or replayed picture | After-action drafting, scheduled products |
| Latency | Interactive, streamed | Minutes to hours; nobody is waiting |
| Availability | Now | After the API transport (GAP-041) |

The desktop-side assistant can be built first, against the embedded services. The
node-side one waits on the transport.

## 2. After-action drafting runs on the node, in batch

Decided here rather than left open: **the node, using the batch endpoint.**

| Reason | Detail |
|---|---|
| Nobody is waiting | A shift report is produced after the shift; interactive latency has no value |
| Cost | Batch processing runs at half the standard rate, and a narrative over a whole session is the largest single request the assistant makes |
| The data is already there | The node holds the authoritative journal; drafting on the desktop means moving the session to the desktop first |
| It does not compete with the operators | A long synthesis on the node cannot slow a desktop mid-raid |

Shape: at the end of a session the node enqueues one request per report section with a
`custom_id` per section, polls until the batch ends, and keys the results by `custom_id`
because **batch results arrive in any order**. Each result is checked for a refusal
individually; the batch endpoint does not support the server-side fallback parameter, so a
declined section is reported as undrafted rather than silently missing.

The figures in the report are computed by `gungnir-reporting` from the journal, before and
independently of any drafting. The assistant writes the narrative around them and never
produces a number (`tools.md` §3).

## 3. Peer systems

A peer command-and-control system reaching the node through the v1 API **may not** ask the
assistant anything in this revision. The reasons are not incidental:

- Authorization is per operator, and a peer system is not an operator with a role.
- The audit entry names an operator; a peer question would have no accountable asker.
- The egress policy is written for a deployment's own data leaving to a model provider,
  not for a peer's question arriving.

If a customer needs it later, it is a new design with its own authorization model, not a
new endpoint on this one.

## 4. Caching across sessions

The system prompt and the tool list are stable per role and per version, which is what
makes the cached prefix work. On the node:

- One prefix per role, warmed at the start of a batch run.
- The prefix changes only when the prompt or the tool list changes, which is a release
  event and is visible in the version.
- Cache hit rates are logged; a run whose reads stay at zero is a defect, not a cost
  variance, and is investigated as such.

## 5. Quotas and budgets

| Control | Where |
|---|---|
| Per-role quota | The loop, on the desktop and the node |
| Per-session cost cap | The loop; reaching it ends the assistant for that session with a message |
| Per-node monthly cap | Configuration; a batch run that would exceed it does not start |
| Usage per exchange | Recorded in the audit entry, so cost is attributable to a role, a session, and a purpose |

## 6. Observability

The assistant reports through `gungnir-observability` like any other subsystem: whether a
provider is reachable, the current cache hit rate, quota consumption, and the count of
refusals and tool errors. A provider that has been unreachable for a configured period
raises an alert through the normal lifecycle, because an assistant that is quietly absent
looks the same as one that has nothing to say.

## Traceability

`architecture.md` for the loop; `provider-configuration.md` §5 for cost control;
`../gungnir-api-v1.md` for the interface the node exposes; gaps GAP-041 and GAP-044.

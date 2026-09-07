# Security

Status: first draft, 2026-09-04. **Human-owned** (`../agentic-workflow.md`): the security
reviewer signs this off and runs a red-team pass before the assistant is enabled in any
profile.

## 1. Threat model

| Id | Threat | Likelihood | Impact | Control |
|---|---|---|---|---|
| T-1 | **Prompt injection** through sensor free text, peer messages, annotations, or report text redirects the assistant | High. These fields are adversary-writable and this is the defining attack on tool-using assistants | Medium. It cannot act, so the worst case is a wrong answer to a person who can check it | Untrusted text delimited as data; frozen cached system prompt; structured tool results; the operator channel is the mid-conversation system message, unreachable from tool output; seven injection cases at a threshold of every case, every run |
| T-2 | **Data exfiltration** through the prompt: restricted content leaves the host to a model provider | Medium | High under the US jurisdiction decision, where operational data may be controlled | Per-profile egress policy checked on the assembled request, not on the intent; allow-listed view kinds on-prem; nothing at all in disconnected and air-gapped profiles |
| T-3 | **Over-trust**: a fluent answer is believed over the panel | High. This is the most likely real harm | Medium to high; a decision taken on a confident wrong summary | Provenance on every answer; links into the panel that holds the figures; no confidence score on prose; no proactive interjection; measured in usability round 2 |
| T-4 | **Credential compromise**: the API key leaks | Low | High | Key in the operating system's secret store; never in a prompt, tool argument, log, audit entry, or error message; rotation is a configuration action; no key at all in no-egress profiles |
| T-5 | **Local model supply chain**: tampered open-weight files | Low to medium | High; the weights determine every answer in the disconnected profile | Pinned version, hash verified before load, no automatic download, licence recorded; treated exactly as plan 09 treats model artefacts |
| T-6 | **Log and audit exposure**: prompts and answers contain operational detail | Medium | Medium | Audit entries hold a digest of tool results rather than full payloads; the audit log inherits the journal's protection and retention; prompts are not written to application logs |
| T-7 | **Denial**: the provider is unavailable or the quota is exhausted mid-raid | Medium | Low. The assistant is a convenience | Honest failure states; the system's function does not depend on it; an unreachable provider raises an alert rather than failing silently |
| T-8 | **Authority escalation by feature creep**: a future "just let it acknowledge alerts" | Medium over time | High; it would breach CAP-4.3 | Two mechanisms: no tool class exists for it, and the crate cannot reach the state-changing crates; a dependency-list test enforces the second |

T-8 is the one to watch, because it arrives as a reasonable request from a real operator
rather than as an attack.

## 2. Prompt injection, in detail

The rule is that **untrusted text is data**. Mechanically:

1. Tool results are structured values wherever the schema allows; free text is a labelled
   field inside them, not the whole result.
2. Untrusted content is wrapped in a delimited block that the frozen system prompt
   identifies as data that may contain instruction-like text to be ignored.
3. The system region is never assembled from anything a sensor, peer, or annotation
   supplied. It is frozen per role and cached, so a change to it is a code change.
4. Operator instructions arriving mid-conversation use the API's mid-conversation system
   message rather than being concatenated into the user turn, which keeps the operator
   channel distinct from content.
5. Drafts are checked for instruction-like content before they are shown, because a draft
   that a later turn reads is a second-order injection path (case EV-inj-07).
6. The assistant's answer cannot cause a tool call outside the loop's own tool list, and
   the list is fixed per role rather than assembled per question.

What is **not** claimed: immunity. Injection defence in a tool-using assistant is
mitigation, not a solved problem. The reason the residual risk is acceptable here is T-1's
impact column: the assistant cannot act, so a successful injection produces a wrong answer
in front of someone who can open the panel and check.

## 3. Egress, in detail

The check runs on the fully assembled request immediately before it leaves the process:

| Profile | Rule |
|---|---|
| Cloud connected | Live picture permitted (D-14) |
| On-prem with egress | Allow-listed view kinds only: summaries, scores, the question. Not raw detections, not journal envelopes, not operator identities |
| On-prem without egress, disconnected, air-gapped | Nothing leaves; local provider or no answer |

A denied request is audited with the reason and never partially sent. The allow-list is
configuration, versioned and audited like any other baseline section.

## 4. What is logged, and where

| Content | Destination | Retention |
|---|---|---|
| Operator, mission time, model, tools called, digest of results, the answer | `gungnir-security` audit log | The audit retention policy |
| Usage and cost per exchange | Audit entry | As above |
| Provider reachability, cache hit rate, refusal and error counts | `gungnir-observability` | Health and alerts |
| Full prompts and full tool payloads | **Nowhere by default** | A debug mode may capture them locally, off by default, and never in a deployed profile |

## 5. Reviewer's checklist

1. The tool list: every entry is read-only or draft-only, and no state-changing path
   exists.
2. The dependency-list test actually fails when a forbidden crate is added.
3. The egress check runs on the assembled request, including tool results, and its denial
   path leaks nothing.
4. Credential handling from the secret store to the request, including error paths.
5. The injection cases, plus a red-team pass adding cases the set does not have.
6. Audit completeness: an answer that reached an operator is always in the log, including
   refusals and failures.
7. Local model provenance and licence.
8. Whether the export determination (D-B8) covers prompts and completions containing
   operational data.

## Traceability

`safety-boundaries.md`; `../architecture/uaf/security/Sc-Tx.md`;
`../release-governance.md`; `../business/open-questions.md` D-B8; D-14; capability CAP-6.7.

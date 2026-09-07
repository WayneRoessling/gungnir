# UI integration

Status: first draft, 2026-09-04. Where the assistant appears, following the designs in
`../ux/`. Plan 06 principle 7 governs: **AI assistance is labelled, sourced, and never has
authority.**

## 1. The assistant panel (PN-19)

Wireframe: `../ux/wireframes/WF-19-assistant.puml`. It is a panel, not a mode: it never
takes the viewport and never covers the approval queue.

| Element | Content |
|---|---|
| Header | Provider and model (`cloud, claude-opus-5` or `local, <model>`), the egress rule in force for this profile, and the line "this panel cannot change anything" |
| Exchange | The question; then the provenance line (snapshot time, tools called, model); then the answer, streamed |
| Draft | Marked `DRAFT, not a record`, with a single control that copies it into the panel that owns it |
| Controls | The question box, an optional "include selected track" checkbox, and navigation links into other panels. **No control that changes state** |
| Empty and failure states | "No provider reachable", "the model declined: \<category\>", "budget reached for this session" |

The provenance line appears **before** the answer streams, so the operator sees the model
and the tools before they read a word. That ordering is deliberate: provenance after the
answer is read after the answer has landed.

## 2. Inline affordances

The assistant appears in three other places, always as an explanation of something the
panel already shows, never as a new number:

| Where | Affordance | Behaviour |
|---|---|---|
| Recommendation panel (PN-05) | "explain this plan" | Opens the assistant with the plan in context; the answer cites the verdict, rationale, and alternatives the panel already has |
| Evidence card (PN-04) | "explain this score" | Same, for the score factors |
| Alerts panel (PN-08) | "summarize these incidents" | Same, for the open incidents |

An inline affordance never rewrites the panel's content. It opens a conversation beside it.

## 3. Streaming and waiting

- Text renders as it arrives; the panel shows a thinking indicator while the model reasons,
  which is why the provider asks for a summarized rather than hidden reasoning display.
- A turn that is still running when the operator navigates away keeps running and the
  answer appears in the panel; it does not interrupt.
- Nothing about the assistant blocks the tick. It runs off the render path entirely.

## 4. Per role

The panel is in every role's layout as an on-demand panel
(`../ux/information-architecture.md` §4), with that role's tool list
(`tools.md` §5). The analyst's is journal-scoped; the operator's is snapshot-scoped.

In the disconnected profile the header says so and the reduced capability is stated
plainly: "local model; journal search unavailable".

## 5. What the design refuses

- **No suggestion chips that pre-write engagement questions.** The assistant does not
  propose what to ask about a decision the operator is about to make.
- **No proactive interjection.** It answers when asked. An assistant that volunteers during
  a raid is competing with the queue for attention.
- **No agency language.** It "found", "summarized", "drafted". It does not "think",
  "believe", "recommend an engagement", or "decide".
- **No confidence score on prose.** A percentage next to a sentence invites exactly the
  over-trust the evaluation set is designed to measure.
- **No "apply" control anywhere in the panel.** An early sketch had one; the plan 06
  walkthrough removed it, and its absence is the design.

## 6. Accessibility

Follows `../ux/accessibility.md`: the panel is keyboard operable, the provenance line is
part of the reading order rather than a tooltip, streamed text does not steal focus, and
the draft marker is text rather than colour alone.

## Traceability

`../ux/wireframes/WF-19-assistant.puml`; `../ux/information-architecture.md` §3 and §4;
`../ux/README.md` principle 7; `safety-boundaries.md` §9; gap GAP-044.

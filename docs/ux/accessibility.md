# Accessibility

Status: first draft, 2026-09-04. What the designs commit to within what egui 0.29
offers, and what is out of reach without a change of toolkit.

## Contrast

- Every text-on-background pair in `design-system.md` meets a contrast ratio of at
  least 4.5:1 at 14 pt and 3:1 for the 12 pt provenance text against the surface it
  is drawn on. There are three surfaces, all tokens since 2026-09-06:
  `VIEWPORT_BACKGROUND` (8, 16, 24) for the map, `PANEL_BACKGROUND` (11, 23, 33) for
  the panels, `APP_BACKGROUND` (7, 14, 21) behind both. `gungnir-ui`'s tests hold
  fifteen text colours to 4.5:1 on all three. Until the theme was installed the panels
  sat on egui's stock grey and nothing checked contrast there; the first draft of this
  page said egui derived the panel colour from the viewport's, which it did not, and
  two colours (`ALERT_COLOR`, and the strip's own red) were under the rule on a panel.
- Health and status chips carry their word, so contrast failure of a chip colour
  degrades to readable text, never to an unreadable dot.

## Colour-independent encoding

- Classification is frame shape plus colour (DS-03); lifecycle is fill plus the
  status word in the table; staleness is grey plus an age label; severity is a bar
  plus the word. No meaning is carried by colour alone, so the designs work for
  operators with colour-vision deficiency and on a washed-out screen.
- Red and green never appear as the only difference between two states (health
  dots carry a tooltip word and the strip carries the flag name).

## Keyboard operation

- Every decision dialog is reachable and operable by keyboard: Tab order is
  verdict, rationale, reason field, then the three decision buttons with Reject
  first and Accept last; Enter never activates Accept (principle 2); Escape closes
  without a decision.
- The queue is navigable with arrow keys; selecting an item with Enter opens the
  recommendation, not the dialog.
- Global shortcuts are few and never single letters: acknowledge alert
  (Ctrl+Shift+A), open assistant (Ctrl+Shift+Space), focus queue (Ctrl+1), focus
  map (Ctrl+2). They are listed in the strip's help.

## Screen readers

- egui 0.29 exposes an accessibility tree through AccessKit on Windows when
  enabled; the designs label every widget (`ui.label` text is the accessible name;
  buttons carry their verb and object, "Reject plan 42"). Live regions for the
  queue and alerts are not available; the strip's counts are read on focus.
- Target: an administrator or analyst can complete every offline task with a screen
  reader; live engagement tasks are not designed for screen-reader operation and
  the usability plan says so.

## Motion and timing

- No content flashes; the only motion is the 200 ms queue reorder, which can be
  disabled in the configuration baseline.
- Time-limited items (plans that expire) show time remaining continuously; nothing
  depends on reacting to a transient message.

## Text and language

- Labels use the display vocabulary from `../mission/glossary.md` with the
  per-deployment override table (D-12, GAP-070); abbreviations expand on hover.
- Numbers keep their units on screen ("34 s", "1.2 km").

## Out of reach in egui today

- Screen-reader announcement of queue changes (no live regions).
- High-contrast OS theme inheritance (egui draws its own theme; the night variant is
  the substitute).
- Per-user font scaling beyond egui's global zoom (which is supported and is the
  recommended mechanism).

## Traceability

- Principles 2, 3, 4; MOP-37 (usability measures include error rate under the
  keyboard-only condition); plan 06 acceptance.

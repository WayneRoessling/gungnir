# Design system

Status: first draft 2026-09-04; second revision 2026-09-06, when the theme became an
installed style rather than a set of constants the panels drew on egui's stock grey;
third revision 2026-09-08 (GAP-095), when the tokens became fields of a `Palette`
value so a night variant could exist. Built on `gungnir-ui/src/theme.rs`: the tokens
are `Palette`'s fields (`palette.app_background`, and so on -- lower-cased from the
constant names below, which this document keeps in their original case as the design
vocabulary), and `install_egui_theme` hands egui's own chrome the same tokens from
whichever `Palette` a deployment resolved at start-up. Nothing here replaces an
existing token's *name*, so `gungnir-viewport3d::materials` keeps reading the same
palette; several values changed on 2026-09-06 and the table says which.

## DS-01 Tokens that exist (`gungnir-ui::theme`)

### Geometry and typography

| Token | Value | Use |
|---|---|---|
| `PANEL_SPACING` | 8 | gap between panels and between rows of controls |
| `ROW_SPACING` | 4 | vertical gap between items inside a panel |
| `DEFAULT_FONT_SIZE` | 14 | anything a decision is made on |
| `SMALL_FONT_SIZE` | 12 | provenance, history, footnotes |
| `TITLE_FONT_SIZE` | 16 | a panel's title, nothing else |
| `TRACK_TABLE_MAX_HEIGHT` | 320 | the track table's scroll area |
| `STATUS_DOT_RADIUS` | 5 | health dots |
| `DASHBOARD_DEFAULT_WIDTH` | 420 | the docked workspace's starting width |
| `TAB_BAR_HEIGHT` | 22 | the dock's tab bar |
| `STROKE_HAIRLINE` | 1.0 | grid, coverage rings, the uncertainty ellipse, borders |
| `STROKE_EMPHASIS` | 1.5 | heading vectors, classification frames, fences, warnings |
| `STROKE_HAZARD` | 2.5 | hazards and barriers (DN-14) |
| `STROKE_GAP_PARTIAL`, `STROKE_GAP_UNCOVERED` | 2.0, 3.0 | coverage gaps (DN-12); the uncovered one is heavier because it is the one acted on |

### Surfaces, text and interaction (the chrome)

| Token | Value | Use |
|---|---|---|
| `APP_BACKGROUND` | (7, 14, 21) | the window; the dock's tab bar; striped rows; code |
| `PANEL_BACKGROUND` | (11, 23, 33) | a docked panel, a window, a menu, an active tab |
| `PANEL_BACKGROUND_HOVER` | (16, 34, 47) | a widget under the pointer |
| `PANEL_BACKGROUND_ACTIVE` | (18, 48, 63) | a widget being pressed; an open menu |
| `BORDER_SUBTLE` | (28, 51, 66) | hairline borders and separators |
| `TEXT_PRIMARY` | (220, 233, 241) | body text on a panel |
| `TEXT_SECONDARY` (= `MUTED_TEXT_COLOR`) | (145, 169, 183) | secondary text, provenance |
| `FOCUS_COLOR` | (57, 198, 232) | keyboard focus, hover strokes, the text cursor, the dock's drag preview; chrome only, never inside the viewport |
| `FOCUS_FILL_ALPHA` | 72 | the selection fill behind selected text and rows |

### Track lifecycle and health

| Token | Value | Use |
|---|---|---|
| `TRACK_CONFIRMED_COLOR` | (60, 200, 90) | confirmed track |
| `TRACK_TENTATIVE_COLOR` (= `WARNING_COLOR`) | (220, 190, 60) | tentative track |
| `TRACK_COASTING_COLOR` | (200, 100, 40) | coasting track |
| `TRACK_DELETED_COLOR` | (130, 130, 135) | deleted track: on screen for at most one tracker step, then history only |
| `TRACK_STALE_COLOR` | (140, 140, 160) | any stale track, regardless of status |
| `ALERT_COLOR` (= `DEGRADED_COLOR`) | (240, 80, 90) | critical alerts, a denied verdict, a false health flag |
| `HEALTHY_COLOR` (= `TRACK_CONFIRMED_COLOR`) | (60, 200, 90) | a true health flag |
| `WARNING_COLOR` | (220, 190, 60) | warnings and cautions |

The aliases are written as aliases in the code (`pub const HEALTHY_COLOR: Color32 =
TRACK_CONFIRMED_COLOR;`) and a test pins them, so a later edit that separates one has
to say so. Each shared value is a pair of facts that never occupy the same channel
(DS-02): health is a dot or a chip, lifecycle is a glyph fill.

### The viewport

| Token | Value | Use |
|---|---|---|
| `VIEWPORT_BACKGROUND` | (8, 16, 24) | the map: the lowest surface on screen |
| `VIEWPORT_GRID_COLOR` | (31, 56, 70) | range rings and grid |
| `VIEWPORT_TEXT_COLOR` | (205, 222, 232) | range labels, readouts, the status line |
| `INTERCEPT_LINE_COLOR` | (70, 200, 200) | assignment lines; the pre-delegated verdict chip |
| `COVERAGE_COLOR`, `COVERAGE_MAX_ALPHA` | (73, 132, 172), 80 | sensor coverage rings |
| `TERRAIN_ALPHA` | 110 | the shaded terrain under the picture |
| `HAZARD_COLOR` | (190, 120, 220) | hazards and barriers |
| `SELECTION_HALO_COLOR` | (240, 240, 240) | the halo around the selected glyph |
| `CLASS_*_COLOR` | see DS-03 | classification frames |
| `TIME_REMAINING_WARN_S`, `TIME_REMAINING_CRITICAL_S` | 30, 10 | when a time-remaining numeral turns amber, then red |

`track_color(status, stale)` is the one function that decides a track's colour;
stale always wins. The lifecycle word comes from `gungnir_model::Vocabulary`
(GAP-070). `numeral(text)` sets a compared number in the monospace face (DS-05).

Values changed on 2026-09-06 and why: the viewport set moved from neutral charcoal
to blue-black so the cool track, coverage and assignment colours sit in the map
rather than on it; `MUTED_TEXT_COLOR` from neutral grey to blue-grey for the same
reason; `ALERT_COLOR` from (220, 60, 60), which reached only 4.3:1 against the map
and 4.1:1 against a panel, to a red that holds 4.5:1 on both; `TRACK_DELETED_COLOR`
likewise from (120, 120, 120); `INTERCEPT_LINE_COLOR` from a blue ten units from the
friendly frame's to a teal; `COVERAGE_MAX_ALPHA` from 110 to 80 so a ring never
competes with the tracks inside it. The status strip's own four colours (a darker
green, a red at 3.2:1, an amber and a grey defined in `status_strip.rs`) were
removed and the strip reads the theme's.

## DS-02 Colour semantics

Two independent encodings apply to every track and are never mixed in one channel:

| Meaning | Channel | Values |
|---|---|---|
| Lifecycle and freshness | glyph fill | the five `TRACK_*` colours; stale overrides |
| Classification (affiliation) | glyph frame shape and frame colour | see DS-03 |
| Health | dots and strip chips | `HEALTHY_COLOR`, `DEGRADED_COLOR`; never a third colour |
| Alert severity | left bar of the alert row | Info grey, Warning `WARNING_COLOR`, Critical `ALERT_COLOR` |
| Policy verdict | chip on the plan | Requires approval amber; Denied red with the reason; Approved (pre-delegated) teal `INTERCEPT_LINE_COLOR` with the delegation named |
| Weapons control status | strip chip | Free red, Tight amber, Hold green, each with its word |
| Backend | strip chip | Node connected green; Embedded grey; Detached after fallback amber with counts |

Red is reserved for "unhealthy, critical, denied, weapons free". It is never used
for hostile classification alone, so a red screen means something is wrong with the
system or the decision, or that engagements may proceed without a person, not merely
that the enemy is present.

The weapons control row was corrected on 2026-09-06 to match what `status_strip.rs`
has drawn since GAP-072: *free* is the permissive state, the one in which an
engagement can happen without an approval, so it is the one that takes the red;
*hold* is the safe state. The first draft had them the other way round.

## DS-03 Classification frames

Affiliation follows the public military symbology convention of frame shape plus
colour so that colour is never the only cue:

| `Classification` | Frame | Frame colour |
|---|---|---|
| Hostile | diamond | `CLASS_HOSTILE_COLOR` (230, 80, 80) |
| Friendly | rounded rectangle | `CLASS_FRIENDLY_COLOR` (90, 170, 240) |
| Neutral | square | `CLASS_NEUTRAL_COLOR` (100, 200, 120) |
| Unknown | quatrefoil (four-lobed) | `CLASS_UNKNOWN_COLOR` (230, 200, 90) |

The fill inside the frame stays the lifecycle colour; a stale hostile is a diamond
with a grey fill. Confidence below the policy margin draws the frame dashed.

## DS-04 Track glyph iconography

| Element | Encoding | Source |
|---|---|---|
| Position | frame centre | `TrackView::position_enu` |
| Velocity | a leader line of length proportional to speed, capped | `state[3..6]` |
| Position uncertainty | an ellipse from the covariance's 2 by 2 horizontal block at one sigma, drawn faint | `covariance` |
| Staleness | grey fill plus an age label "stale 34 s" beside the glyph | `Quality::is_stale`, `mission_time` |
| Association confidence | frame solid above the margin, dashed below | `Quality::association_confidence` |
| Assignment | a teal line in `INTERCEPT_LINE_COLOR` from the resource to the track; the intercept point as a small cross when present | `InterceptSolutionView` |
| Selection | a white halo: white is the one colour no other viewport element uses | UI state |
| Threat score | a small numeral at the frame's upper right, only when above a display threshold | `RiskScore` |

The line colours that share the map (assignment, friendly frame, neutral frame,
coverage, selection) are held pairwise apart by a test in `gungnir-ui`, because two of
them were once ten units apart.

## DS-05 Typography

- One sans-serif family (egui's default); 14 for anything a decision is made on, 12
  for provenance, 16 for a panel's title and nothing else; bold for the item under
  decision (the selected plan's identifier, the alert title).
- Numbers that are compared (time remaining, scores, positions, speeds, ages,
  evidence weights) are set in egui's monospace face at body size through
  `theme::numeral`, because the proportional face has no tabular figures and a column
  of numbers that does not line up cannot be scanned. A number inside a sentence
  ("Speed 12.3 m/s") stays in the sentence's face.
- Time remaining is written as "1:42" (minutes:seconds) and turns amber under 30 s
  and red under 10 s; never as a bare number.
- `install_egui_theme` maps egui's text styles to these sizes: Body and Button 14,
  Small 12, Heading 16, Monospace 14.

## DS-06 Density rules

- Live layouts: 14 pt rows, at most twelve queue rows visible, four-word labels.
- Offline layouts: 12 pt allowed in tables; unlimited rows with virtualisation.
- No panel scrolls horizontally; columns collapse to an ellipsis and expand on hover.
- No animation except the strip's counts changing and the queue reordering with a
  200 ms slide, so a reorder is noticed but never chased. egui's own easing of
  collapsing headers and toggles is switched off by the installer for the same
  reason (`animation_time` is zero).
- No rounded corners and no shadows on windows, panels or widgets: a console is
  edges and surfaces, not cards floating over a page. Tables are striped so a
  twelve-row queue is read across.

## DS-07 Dark operations-room mode

The palette is dark by default and the whole window is one surface set:
`VIEWPORT_BACKGROUND` (8, 16, 24) for the map, `APP_BACKGROUND` (7, 14, 21) behind
everything, `PANEL_BACKGROUND` (11, 23, 33) for the panels one step above it. The
installer (`theme::install_egui_theme`, called once from `gungnir-app`'s creation
closure and from the headless render probe) gives egui's chrome these surfaces, so
the frame around the picture is the same operations-room surface as the picture.

A "night" variant exists as a second `Palette` value, `theme::Palette::night()`: the
chrome's surface, text and interaction hues (`APP_BACKGROUND` through `FOCUS_COLOR`)
at 70 percent of their day WCAG relative luminance, `ALERT_COLOR` unchanged, the grid
(`VIEWPORT_GRID_COLOR`) darker still at that same factor applied twice (49 percent of
day). Everything else this table lists -- lifecycle, classification, coverage,
hazard, selection, geometry -- is outside that scope and is the same value in both.
Switching is a `ConfigBaseline` setting (`UiSettings::theme`, `"day"` or `"night"`,
validated by `gungnir-config` and resolved once into `AppState::palette` at
start-up), not a per-session toggle: there is no control anywhere that changes it
once a session has started, so a shift never inherits a surprise. **Built
2026-09-08**, GAP-095 (filed 2026-09-06 as GAP-090, renumbered the same day to
GAP-094 when GAP-090 was taken by an unrelated gap out of DN-25, and renumbered again
2026-09-07 on discovering GAP-094 was by then also carrying the advisories-gate gap
-- a second collision this document did not catch, found and fixed in review rather
than at the time). The 2026-09-06 decision (D-35, filed that day as D-33 and
renumbered for the same reason) had kept the tokens as flat constants until the
variant was scheduled, because threading a `Palette` value through the call sites
across three crates -- 404 of them, by GAP-095's own count, not the 239 estimated at
the time -- was its own tranche; GAP-095 is that tranche.

## DS-08 Iconography for the strip and panels

Text chips, not icons, for everything with a meaning that must be read aloud over a
radio: "Hold", "Tight", "Free", "Detached", "Replay". Icons only for panel headers.

## Theme changes, as built

The classification constants, the warning colour, the selection halo, the
time-remaining thresholds, `classification_color` and `classification_frame` were
built 2026-09-05 under GAP-073 (`ux-to-code-map.md`); the enum is spelled
`ClassificationFrame` in code, leaving the name `Frame` to egui.

The surface, text and interaction tokens, the stroke tokens, `numeral`,
`operations_visuals` and `install_egui_theme` were built 2026-09-06 (ARCHITECTURE.md
§10 item 89), together with the value changes DS-01 lists, the `Deleted` mapping
(`track_color` once returned the stale grey for a deleted track, which told an
operator a removed track might still be there) and the dock's tab-bar hooks in
`gungnir-app/src/dock.rs`.

The night variant, the `Palette` struct that carries both it and the day values, and
the `ConfigBaseline` setting that picks between them were built 2026-09-08 (GAP-095,
ARCHITECTURE.md §10 item 112): every call site that used to read a flat constant now
reads a `Palette` field or calls a theme function taking `&Palette`, threaded
explicitly from `AppState::palette` rather than through a global.

Still not built: the dashed low-confidence frame, which needs the policy margin
`gungnir-policy` owns; a threshold invented in the viewport would mark tracks
low-confidence against a rule nobody set.

## Traceability

- Principles 3 and 5 (`README.md`); accessibility (`accessibility.md`) constrains
  every colour pair to the contrast rule there, now against every surface.
- Capabilities CAP-2.2 (stale visible), CAP-2.6 (classification with confidence),
  CAP-4.1 (verdict visible).

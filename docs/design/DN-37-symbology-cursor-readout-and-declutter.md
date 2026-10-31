# DN-37 Symbology, cursor readout and declutter

GAP-227's three parts beside the tiles of `DN-36-staged-terrain-and-map-tiles.md`: D-139's
MIL-STD-2525D symbols, a cursor readout in MGRS and latitude and longitude, and declutter
(leaders and label placement). D-163 lets them proceed in parallel with the tiles. Status:
design, 2026-10-30; **nothing is built**. The owner walked eight choices the same day, D-165
to D-172, and took every one to its recommended default; §8 records them. What the owner has
signed of this note is in [`../signatures.md`](../signatures.md).

## 1. What it is for

`../review/2026-10-26-comprehensive-review.md` §8 finding 6 found the picture drawn in glyphs
of this system's own making, with no position readout an operator could pass on, and labels
that overprint where tracks bunch. `../review/2026-10-30-ui-review.md` confirmed the overprint
on screen, at tracks 41 and 42. A defence operator reads MIL-STD-2525D symbols, gives
positions in MGRS, and works a dense picture. Each of the three parts below answers one of
those, and each is drawn by the one function both renderers share, so the scene and the
projection cannot disagree.

## 2. What exists

Read from the code at `46e7a1ac`:

| What | Where | What it gives this note |
|---|---|---|
| Identity | `gungnir_model::Classification`: Unknown, Neutral, Friendly, Hostile | Four states. D-136 widened the type to STANAG 1241's nine; that is GAP-216, **Open and unbuilt** |
| Battle dimension | Nothing. `TrackView` carries no air, surface or subsurface | A 2525D frame's shape depends on it (§3.2) |
| Platform class | `gungnir-app/src/adsb.rs::platform_class_of_category` (`air.crewed`) and `cooperative.rs::platform_class_of_ship_type` (AIS's `surface.*`), on cooperative identity only | The only class the picture holds; the icons it can draw (§3.4) |
| The glyph | `gungnir-viewport3d/src/tracks.rs::draw_glyphs_2d`: `theme::ClassificationFrame` (diamond, rounded rectangle, square, quatrefoil) round a small fill in the **lifecycle** colour, the one-sigma outline, a `HEADING_VECTOR_SECONDS` (10 s) heading line, and the id label at a fixed offset (`id_label_anchor`) | What the symbols replace |
| The scene's glyph | `gl::instances`: one disc per track in `theme::track_color`, the lifecycle colour; GAP-251 draws the projection's glyph over it in egui | The disc becomes the symbol's fill (§3.3) |
| Coordinates | `gungnir_model::LocalFrame::to_geodetic`, through `gungnir-coord`'s oracle-verified transforms | No UTM, UPS or MGRS anywhere in the workspace |
| Ground height | GAP-247's `HeightField` over the placed DEM | The readout's ground (§4) |

## 3. Symbology (D-139, D-165 to D-168)

### 3.1 The code, then the drawing

A pure function builds each track's **symbol identification code** (SIDC), as 2525D defines
it: standard identity, symbol set (the dimension), status, and the entity, its type and
subtype. It lives in a new `gungnir_model::symbology` module, because it is data, not drawing.
The interop bearers can then use the same code later. The viewport draws from the SIDC alone,
so a symbol on screen is exactly what its code says.

### 3.2 What the code is built from

- **Standard identity (D-165).** The symbols are built now for the four identities a track holds:
  Unknown, Friend, Neutral and Hostile. When GAP-216 widens the type, Pending, Assumed Friend
  and Suspect take their own codes. Joker and Faker take 2525D's exercise treatment. Nothing
  else changes, because the drawing reads the code, not the enum. The renderer is written once,
  for all nine.
- **Dimension (D-166).** `TrackView` gains `dimension`: Air, Sea Surface, Sea Subsurface, Land
  or Unknown, defaulting to Unknown, carried under the schema's compatibility rule. The tracking
  service sets it from the sources that formed the track, **where a source says so**. AIS says
  sea surface, and ADS-B and Remote ID say air, each by its protocol. A radar says nothing until
  its detection model states the dimension it observes, which none does today. Sources that
  disagree leave it Unknown. So round 1's radar-only tracks are drawn with an unknown dimension
  until their detection models gain one, which is stated, not guessed. An Unknown dimension
  takes the frame 2525D gives a symbol whose dimension is unknown. The build confirms that frame
  against the standard's own figures; if the standard gives none, D-166 returns to the owner.
- **Status (D-167).** Lifecycle moves out of the fill and into the frame, as 2525D carries it:
  - a tentative track has the standard's dashed frame, as anticipated;
  - a confirmed one has a solid frame;
  - a coasting or stale one is faded, with "stale" as a text amplifier.

  The fill takes the standard's affiliation colour, and the track table keeps its lifecycle
  colours. In the scene, the GL disc takes the same fill colour, so the disc and the egui
  frame over it agree. That is a change to `gl::instances`, which no test can see, so the
  owner's look covers it (§6.2).
- **Entity and icon (D-168).** The icon comes from the platform class where the track has one.
  `air.crewed` and each AIS `surface.*` class map to their 2525D entities, and the mapping is a
  table with a test per row. A track with no class, or a class with no row, shows its frame
  with no icon, never a guessed one. Echelon and unit modifiers stay blank (D-139).

### 3.3 Drawing (D-168)

The frames, their status variants, the amplifier field positions and the icons are drawn
in-house as egui shapes, from the SIDC, by one function both renderers call (`draw_tracks`,
GAP-251). There is no dependency, and `RenderProbe` sees every shape, so each variant is
tested headless. The amplifiers the picture already carries go in 2525D's own fields: track
number, speed, altitude, quality and the stale text. The direction-of-movement line is the
leader (§5.1). The night palette leaves classification colours alone, as D-35 does today.

## 4. The cursor readout (D-169, D-170)

While the pointer is over the viewport, its bottom-right corner reads, for the point under it:

```
10T DQ 12345 67890   43°40'12.0"N 123°15'00.0"W   ground 165 m NAVD88
```

- **MGRS to 1 m** (10 digits) and **latitude and longitude in degrees, minutes and seconds**,
  WGS 84 (D-169), from the cursor's ENU through `LocalFrame::to_geodetic` and the conversion
  below.
- **The ground under the cursor**, from the height field, when terrain is loaded. It is given
  in the vertical system the terrain declared, or as height above the ellipsoid, labelled, when
  it declared none. Past the terrain the field is left out, never zero.
- **With no origin declared**, the readout says the picture is in local metres only and can
  give no geographic position, rather than showing one from a guessed origin. That is the same
  rule the coverage layer follows (`layers.rs`'s module documentation).

**MGRS is built in-house (D-170)**, in `gungnir-coord` beside its other transforms: UTM by the
Krüger series, UPS for the poles, and MGRS's lettering with the Norway and Svalbard zone
exceptions. It is held to GeographicLib's `GeoConvert` on committed test vectors, generated at
a pinned version and described in a `SOURCE.md`. The vectors cover zone and band edges, the
exceptions and both poles. The criterion is agreement to a millimetre in UTM's domain and an
identical MGRS string. `gungnir-model` exposes it through `LocalFrame`, so the viewport needs no
new dependency edge.

## 5. Declutter (D-171, D-172)

### 5.1 Leaders

The leader is the direction-of-movement line, drawn to where the track will be after the
**leader time**. The fixed 10 s, which shrinks to nothing zoomed out, gives way to an
operator-selected time (D-171): 10 s, 30 s, 1 min, 2 min or 5 min, default 1 min. It is a
control beside the 2D/3D corner control, reading "leaders 1 min". It is session state, like the
renderer choice, so the next launch starts from the default. Every leader on screen means the
same thing at every zoom.

### 5.2 Labels

A track's label is its text block: the track number with its assignments on one line (GAP-253),
then its amplifiers. A pure function places all of them each frame (D-172):

1. **Priority**, highest first: the selected track; hostile tracks, and once GAP-216 lands,
   suspect, joker and faker; tracks with an assignment; the rest. Ties are broken by track
   number, so the order is stable.
2. **Placement.** In priority order, each label takes the first free of eight positions round
   its symbol, starting at today's (up and right). It keeps last frame's position while that is
   still free, so labels do not jump as tracks move. A label away from its default position is
   joined to its symbol by a thin leader line.
3. **Collapse.** A label with no free position, and lower in priority than what holds the
   space, collapses into a count, "+3", drawn once at the cluster. **A hostile's label is never
   collapsed.** Where two hostile labels have no free position, both are drawn, overlapping:
   hiding either would hide a threat, and the overlap itself shows that the cluster needs a
   closer look.

Placement uses a coarse screen grid to find neighbours, so it stays linear in the track count at
Scenario 4's numbers. It is timed in `gungnir-app/tests/frame_budgets.rs` against the egui
budget.

## 6. What can be tested headlessly, and what only the owner can see

### 6.1 Headless

Drafted as rows for `../verification-capability-table.md` §2. A pass criterion is
human-owned; these are drafts until the owner agrees them, and are not gates.

| Row | Test | Criterion |
|---|---|---|
| a. The code | `gungnir-model::symbology` unit tests | Every identity, dimension, status and mapped class gives the 2525D code the standard's tables give; an unmapped class gives a code with no entity |
| b. The drawing reads the code | `RenderProbe` | Each identity's frame, each status's frame style and each mapped icon is drawn; a track with no class draws a frame and no icon; both renderers draw the same shapes over a track |
| c. Dimension from sources | `gungnir-tracking-service` | AIS alone gives sea surface, ADS-B alone gives air, a radar alone gives Unknown, two sources that disagree give Unknown |
| d. MGRS | `gungnir-coord` against the committed `GeoConvert` vectors | Identical MGRS strings, and UTM to a millimetre, across zone and band edges, the Norway and Svalbard exceptions and both poles |
| e. The readout | `RenderProbe` | The corner reads the MGRS, DMS and ground the conversion gives for a cursor position; with no origin, the sentence instead of a position; past the terrain, no ground |
| f. Leaders | `RenderProbe` | A leader's length on screen is speed times the selected time over the zoom; the control's text says the time |
| g. Labels | `place_labels` unit tests and `RenderProbe` | No two drawn labels overlap except hostile ones; each collapsed label is counted in exactly one "+n"; no hostile label is collapsed; a label keeps its position across frames while it stays free |
| h. The budget | `frame_budgets.rs`, release | Label placement at Scenario 4's counts, measured and stated |

### 6.2 Only on a display

1. Each track wears its 2525D frame and fill in both renderers, and switching renderers changes
   nothing about it.
2. A tentative track's frame is dashed, and a stale one is faded and says so.
3. In the scene, the disc under each symbol is its fill colour, and the frame sits on it.
4. The readout follows the pointer, and its MGRS for a known landmark, against the metre grid
   and the origin, matches a published map.
5. In round 1's cluster at tracks 41 and 42, no two labels overprint. Those that move are joined
   to their symbol by a leader line, and a crowded cluster shows "+n".
6. Changing the leader time lengthens every leader at once.

## 7. Interface delta

- `gungnir-model`: `symbology` (the SIDC and its builder); `TrackView::dimension` under the
  compatibility rule; `LocalFrame` exposes MGRS.
- `gungnir-coord`: UTM, UPS and MGRS, with committed `GeoConvert` vectors.
- `gungnir-tracking-service`: sets `dimension` from the contributing sources.
- `gungnir-ui`: 2525D's affiliation fill colours in the palette.
- `gungnir-viewport3d`: the symbol drawing in `draw_tracks`, the readout, the leader control
  and `place_labels`; `gl::instances` takes the fill colour.

**No dependency edge and no new crate**: the viewport reaches `gungnir-coord` through
`gungnir-model`, which already depends on it.

## 8. The decisions, as the owner took them on 2026-10-30

| Id | Choice | Taken |
|---|---|---|
| **D-165** | The order of the symbols and GAP-216 | **The symbols now**, for the four identities, the other five appearing when GAP-216 lands; the default. Rejected: GAP-216 first |
| **D-166** | Where a symbol's battle dimension comes from | **The track's sources**, where they state it, Unknown otherwise; the default. Rejected: air for every track; a dimension per sensor in the baseline |
| **D-167** | What carries the lifecycle once the fill is the affiliation | **The frame's status**, the 2525D way: dashed tentative, faded stale; the default. Rejected: keeping the lifecycle fill inside unfilled frames |
| **D-168** | How the symbols are drawn | **In-house vector shapes** from the SIDC; the default. Rejected: a symbol library or font |
| **D-169** | What the cursor readout shows | **MGRS to 1 m, DMS and the ground**, WGS 84; the default. Rejected: one operator-chosen format; MGRS with decimal degrees |
| **D-170** | Where MGRS comes from | **In-house in `gungnir-coord`**, held to `GeoConvert`; the default. Rejected: adopting a crate |
| **D-171** | How long a leader is | **An operator-selected time**, default 1 min; the default. Rejected: zoom-adaptive |
| **D-172** | What happens when labels would overlap | **Move, then collapse**, a hostile's never; the default. Rejected: move then hide; hide by priority only |

## Traceability

GAP-227; D-136, D-139, D-163, D-165 to D-172; GAP-216, GAP-251, GAP-253; CAP-2.10, CAP-5.9;
PN-02, PN-03; `DN-36-staged-terrain-and-map-tiles.md`; `../review/2026-10-26-comprehensive-review.md`
§8 finding 6; `../review/2026-10-30-ui-review.md`; D-35; `../rust-ui-architecture-coding-standards.md`.

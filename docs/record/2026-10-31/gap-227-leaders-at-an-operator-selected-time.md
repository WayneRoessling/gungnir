# GAP-227: leaders at an operator-selected time (DN-37 increment 2)

D-171, `docs/design/DN-37-symbology-cursor-readout-and-declutter.md` §5.1. The
direction-of-movement line each track drew was a fixed ten seconds of its velocity, which
shrinks to nothing as the picture is zoomed out. It is now the track's **leader**: its velocity
times a time the operator selects, ten seconds, thirty, a minute, two or five, a minute to
begin with.

**What changed.**

- `tracks::LeaderTime` holds the five choices, their seconds and the text the control reads
  (`leaders 1 min`). `draw_glyphs_2d` takes one, in place of `HEADING_VECTOR_SECONDS`.
- `ViewportState::leader_time` is the session's choice, beside `use_3d` and with its rule: the
  next launch starts from the default.
- A control beside the renderer toggle steps through the five on a click. It sits in
  `draw_renderer_toggle`, which both renderers' callers already call, and it is drawn whether
  or not the scene attached, so the projection alone has it.
- `draw_tracks`, the one function both renderers call, passes the choice, so the leader cannot be
  one length in the scene and another in the projection.

**What the tests hold** (`gungnir-viewport3d`):

- `a_leader_is_as_long_as_speed_times_the_selected_time_over_the_zoom` (row f): a 5 m/s track at
  two zooms and all five times draws one line segment of exactly speed times time over metres per
  pixel.
- `the_leader_times_are_the_five_the_owner_chose_and_default_to_a_minute`: the five times, the
  default, the stepping order and the five texts.
- `the_leader_control_is_in_both_renderers_and_sets_what_both_draw`: through `render` and
  through `prepare_3d` with the toggle, the control reads `leaders 1 min`, a simulated click
  makes it `leaders 2 min` and doubles the leader's drawn length, and the two renderers draw the
  same lengths.

**What follows from the default.** A minute is six times the ten seconds the picture drew
before, so a fast track's leader now reaches far across the picture: a 250 m/s aircraft draws
15 km. That is the owner's choice (D-171); the control is how an operator shortens it.

**What the owner should look at on a display.** The same session as the first look:

```
GUNGNIR_CONFIG=testdata/usability/round-1.json
gungnir-app --rehearsal testdata/usability/round-1-seed.json
```

1. The viewport's top right reads `leaders 1 min` beside the `2D`/`3D` control, in both
   renderers.
2. Clicking it steps through `10 s`, `30 s`, `1 min`, `2 min`, `5 min` and back to `10 s`, and
   every track's leader lengthens or shortens at once.
3. Each leader runs from its track the way the track is moving, and its length is the same
   whichever renderer is in front.
4. Zooming in or out does not change what a leader means: its length in the metre grid stays
   the same, and only its length on screen follows the zoom.
5. At a narrow window the `leaders` control does not overlap the status line or the`n   `2D`/`3D` control.

Nothing here is a GL change: the leader is drawn by egui over the scene's discs, as the
heading vector was. Until the owner has looked, it is unseen on a display.

**Not done.** The leader is still drawn in the track's lifecycle colour; the fill and the
frame take the affiliation colour with the symbols (D-167).

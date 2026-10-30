# The three-d scene draws each track's frame, labels and assignment

The owner's look at the three-d scene (`the-owner-s-look-at-the-three-d.md`) passed, and
recorded what it did not show: the scene drew each track as a flat disc and nothing else. The
2D projection draws, per track, the classification frame round a small lifecycle fill, the
one-sigma outline, a ten-second heading vector, the track number, and the resource an
intercept plan assigns to it. The scene's status line counted those assignments while the
scene drew none of them. That is GAP-251.

**What changed.** `gungnir-viewport3d/src/lib.rs` has one function, `draw_tracks`, that calls
`tracks::draw_glyphs_2d` and `tracks::draw_plan_2d`, and both `render` and `prepare_3d` call
it, the way both call `draw_layers` for the coverage. A detail cannot be drawn in one renderer
and missing in the other without somebody changing that function or one of the two calls.

**The frames sit around the discs.** The disc is the scene's only GL geometry and the thing the
owner looked at, so it stays as it is: lifecycle colour, the 2D glyph's radius. egui draws the
projection's shapes over it. That needed a change to the paint order. egui draws its shapes in
the order they were added, and the paint callback that draws the scene was added after
everything `prepare_3d` painted, so anything `prepare_3d` drew sat under the discs.
`prepare_3d` now adds an empty shape where the scene belongs, after the map, the bearing rays
and the status line, draws the tracks after it, and returns its index in `Prepared3d`. The
desktop sets its callback into that slot with `egui::Painter::set`, so the GL draw runs
between the map and the glyphs.

**What the tests hold.** Two tests in the crate run `prepare_3d` headless, set a stand-in
callback into the slot as the desktop does, and read egui's shape list:

- `the_scene_labels_each_track_and_assignment_over_its_disc`: each track's number and the
  plan's `R7` are painted after the callback.
- `the_scene_draws_every_track_detail_the_projection_does`: the shapes after the callback are
  exactly the shapes `draw_tracks` paints alone, frames, outlines and vectors included.

Both fail if the overlay is removed from `prepare_3d`, and both fail if it is moved ahead of the
slot; each was tried. Neither can see the GL draw.

**One difference remains, and it is the disc's.** Inside the glyph radius the disc fills the
frame, where the projection leaves it empty around a small fill. A heading vector is drawn in
the lifecycle colour, so where it is shorter than the radius (a slow track zoomed out) it lies
on a disc of its own colour and cannot be seen; the projection shows it inside the frame. A
sigma outline smaller than the disc is behind the same fill. Neither the outline nor the
vector is lost once it reaches past the disc. Shrinking the disc to the fill would remove the
difference, but egui's fill would then cover the whole GL draw, and nothing would show whether
the scene had drawn at all.

**What the plan line is.** The task called them assignment lines; the projection has never
drawn a line. `draw_plan_2d` labels each assigned track `R` and the resource number, because an
intercept solution carries no point to draw a line to yet, and the three-d line geometry is
`tracks::update_intercept_geometry`, which still returns `NotImplemented` (`docs/unbuilt.md`).
The scene now shows the same labels.

## What the owner should look at

The same session as the first look:

```
GUNGNIR_CONFIG=testdata/usability/round-1.json
gungnir-app --rehearsal testdata/usability/round-1-seed.json
```

1. Each disc has its track number above and to its right, in the disc's colour.
2. Each disc has a frame drawn over it: diamond for hostile, rounded square for friendly,
   square for neutral, four lobes for unknown. Switched to 2D with the corner control, every
   track has the same frame.
3. A heading line runs from each disc the way the track is moving (west for the seed's
   tracks), and is the same length in both renderers past the disc's edge.
4. Zoomed in until a track's uncertainty is wider than its disc, the rounded outline shows
   round it, the same size as in the projection.
5. Once a plan is decided, each assigned track has `R` and a resource number below and to its
   right, and the count of those labels is the status line's count of assignments.
6. Track 39 going stale at 65 seconds changes its number's and its disc's colour together.
7. Panning and zooming, the numbers and frames stay on their discs, with no lag or offset;
   the same at 100 % and 150 % display scaling.
8. With the window resized and the panels moved, no number or frame is drawn over a panel.
9. The discs still show inside the frames. If only frames show, with no filled disc under
   them, the GL draw has stopped and the overlay is hiding the fact.

Until the owner has looked, this is unseen in the same sense the scene was before 2026-10-30:
the tests see the overlay and the order it is drawn in, and nothing sees the discs under it.

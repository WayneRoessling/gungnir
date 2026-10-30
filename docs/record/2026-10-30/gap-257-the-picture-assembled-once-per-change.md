# GAP-257: the picture assembled once per change, and the 2D path's per-frame allocations

GAP-255 fixed the two GPU and geometry rebuilds of the interface review of 2026-10-30
(`../../review/2026-10-30-ui-review.md` §4.3, findings 14 and 15) and put the viewport
inside the egui gate (finding 17). It deliberately left findings 16 and 18, because the gate
now times them. This item fixes them. The criterion of the `gungnir-ui` row is unchanged.

**Finding 16: one assembly, keyed.** `draw_picture` in `main.rs` rebuilt the coverage
circles, the coverage report, the placed hazards and geofences, the filter predictor's
lines for every track, the laydown preview and the dead-spot label on every frame. The same
sixty lines sat in `draw_detached_panel`, and a third copy in `frame_budgets.rs` was kept in
step by hand. They are now one module, `gungnir_app::picture`. `PictureCache::sync` brings
five layer groups up to date, each against its own key, and `render_projection` draws the
projection from the cache. The docked viewport, the detached viewport and the gate all call
those two functions. The scene branch of `draw_picture` lends the same cache's frame to
`prepare_3d`.

The keys, which are the part to review, since a producer that starts reading something new
has to add it:

- **Coverage** (circles, report, approach names, accepted gaps): the baseline revision, each
  active sensor's coverage region, the terrain mask by identity of the height field, and the
  standing acceptances by count and highest identifier.
- **Rules** (hazards, geofences): the baseline revision, the hazard layer's baseline version
  and its count.
- **Predictions**: the baseline revision, the process noise, and the tracks compared whole.
- **Preview**: the baseline revision and the selected laydown.
- **Dead-spot label**: the baseline revision and the chosen sensor.

A `LayerInputs` is still borrowed. `PictureCache::frame` builds the four small vectors of
views that point into the cache, which is one allocation each; what the cache removes is the
work behind the pointers. The terrain layer and the point clouds were already borrows of the
data store and are borrowed as before.

A test, `the_picture_is_reassembled_only_when_its_inputs_change`, holds the rebuild counts:
a still state computes every group once, a laydown selection recomputes the preview alone, a
chosen sensor the label alone, and moved tracks the predictions and not the rule layers. The
gate asserts the same counts over its 200 passes.

**Finding 18: what §5 forbids, where the picture does not change.**

- Sentences: the status line, the hidden-layers line, the coverage note, the dead-spot line
  and the dense-group and laydown labels were `format!`ed every frame. Each is a
  `CachedText` on `ViewportState` written when the facts it states change. The dead-spot
  line prints its age in whole seconds, so it is rewritten once a second as the field ages.
- Track ids were `to_string()`ed per glyph per frame; they are written with the glyph list.
- The plan's labels were found by a linear search of the solutions per glyph and formatted
  each time; `PlanLabels` writes them when the solutions change and looks a track up by
  binary search.
- Polylines: gaps, hazards, geofences and predictions no longer collect a vector of points
  to walk. Dashes are projected as they are drawn, a hazard's clone is gone, and a sector's
  outline is projected straight into the one vector egui's line owns. Each point is
  projected as often as before, so the picture is the same.
- The binary cloned the whole glyph vector into the paint callback every frame. The list is
  an `Arc` on `ViewportState`, and the callback holds a pointer.

**Left, and why.** The point clouds are up to forty thousand `circle_filled` shapes a frame.
One mesh or a texture would draw square points without antialiasing, which is a different
picture, so it is GAP-258, open, with the owner's look as its check. Finding 16 also named
`prediction::predict` running a second time for PN-04, `asset_exposure` in PN-04, PN-17 and
the warnings tick, and PN-10's `SensorPlanner::recommend` on every visible frame. Those are
panel computations and not the picture's assembly; they are untouched here.

**Residual cost and a risk.** The coverage key reads the sensor registry's coverage regions
every frame, because the registry has no revision to key on; it is a handful of records. The
predictions key holds a copy of the tracks and compares them whole every frame, 212
comparisons at Scenario 4's count and one clone per snapshot that changed. A track holding a
`NaN` never equals itself and predicts every frame, which errs towards drawing, as the glyph
rebuild does. The keys name the baseline by revision, so a change to the baseline that does
not advance its revision would show a stale picture until something else changed; no code
path in the tree does that, and the promotion advances it.

**The figures.** The egui gate (`cargo test -p gungnir-app --test frame_budgets --release`,
Scenario 4, 212 tracks) printed a p99 of about one to two milliseconds for both workspaces
before this change and about one to two milliseconds after it, in alternating runs on one
machine, against the 8 millisecond criterion. The difference between runs of the same code is
larger than the difference between the two, as the hybrid cores of this machine make it
(`../../performance-budgets.md`, "What has been measured"). So the honest reading is that the
assembly was a small part of the frame at this count and the change does not move the gate; what
it does is stop the work being done thirty times a second, make the gate time the code the
desktop runs, and fail if the per-frame reassembly returns.

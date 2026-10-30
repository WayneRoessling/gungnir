# The viewport's two per-frame rebuilds, and the egui gate times the picture

The interface review of 2026-10-30 (`../../review/2026-10-30-ui-review.md` §4.3) found two
rebuilds that `rust-ui-architecture-coding-standards.md` §5 forbids, both running on every
frame whether or not anything had changed, and found that no budget could have caught
either, because the egui gate timed the dock tree and nothing else. Its §6 item 2 puts the
three findings together as one change. That is GAP-255.

**The glyph instance buffers (finding 14).** `SceneRenderer::paint_glyphs` called
three-d's `set_instances` on every frame. In three-d 0.19.0 that call allocates four fresh
`InstanceBuffer`s, three transform rows and the colours, so a still picture of two hundred
tracks was eight hundred buffer allocations a second. The struct's documentation said both
GPU objects were "created once", which was true of the sphere's vertex buffers and not of
the instance data. Everything an instance carries is a function of three things: the glyph
list, the zoom (the radius is sized at it) and the palette, which is fixed at attach. So the
upload is now keyed on the first two. `ViewportState::glyph_revision` changes exactly when
the glyph list is rebuilt, in the one function both entry points now share, and the binary
passes it into the paint callback beside the glyph clone. `gl::GlyphUploadKey` holds the
revision and the zoom's bits, `gl::glyph_upload` is the decision, and the renderer uploads
only when the key it holds differs. A pan changes neither, so it uploads nothing.

**The terrain mesh (finding 15).** `draw_terrain_2d` projected the surface's vertices,
up to 16 384 of them, into an `egui::Mesh` every frame it was drawn. GAP-252 had already
made the colours and triangles once per load; the projection was the part still per frame.
The mesh now lives on `ViewportState::terrain_mesh_2d`, keyed on the surface's revision, the
view and the rect, following `sync_dead_spot_texture` as the model: synced once a frame
before the layers are drawn, reprojected only when the key changes, dropped when there is
no surface. It is held behind an `Arc`, because `egui::Shape::Mesh` takes one, so the frame
that draws it clones a pointer. The scene drops it while it draws its own ground, rather
than keeping a projection current for a frame that does not draw it. `draw_terrain_2d`
refuses a mesh whose key is not this frame's instead of reprojecting on the spot: the
paint-order tests of GAP-252 see a missing ground, where a silent reprojection would have
put the per-frame build back.

**The egui gate (finding 17).** `egui_pass_samples` timed `tree.ui`. It now draws the frame
as `main.rs` lays it out: the dashboard on the left with the dock tree, and the picture in
a central panel through `gungnir_viewport3d::render`, from the same layer inputs
`draw_picture` assembles, assembled per pass as the desktop assembles them. A pass whose
frame never draws the viewport's status line fails, as a pass that never drew the track
table already did, and a picture short of Scenario 4's two hundred glyphs fails too. The
row's criterion, p99 under 8 ms at Scenario 4 counts, is unchanged, and it is asserted in
release only as before. What changed is how much of the frame is held to it. The budget
table's note scopes the 8 ms to the panels, with "the rest for the viewport and
`update()`"; the gate now holds the panels and the picture together to the panels' figure,
which is stricter than the budget, not wider.

**The tests.** In `gungnir-viewport3d`: `the_instances_are_uploaded_once_per_glyph_list_and_zoom`
and `a_nan_zoom_does_not_upload_every_frame` in `gl.rs`, pure, without a GL context, as the
ground pass's upload test is; `the_terrain_mesh_is_projected_only_when_its_inputs_change`
in `layers.rs`; and in `lib.rs`, through the frame,
`the_glyph_revision_changes_only_when_the_glyphs_are_rebuilt` and
`the_projection_reprojects_the_terrain_only_when_the_view_moves`. The GPU half of the
glyph fix has no test, for the reason `gl.rs` gives for every GL call: there is no headless
GL context. The decision is tested; the call it guards is read.

**What this does not do.** Finding 16, the layers reassembled twice a frame in the binary,
and finding 18, the 2D path's per-layer allocations, are untouched; the gate now times both,
which is the point of extending it before fixing them. The timing figures this test prints
are not reported here: this machine's hybrid cores vary them about two times by core class
(`../../performance-budgets.md`, "What has been measured").

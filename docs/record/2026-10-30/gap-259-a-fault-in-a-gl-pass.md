# GAP-259: a fault in a GL pass is caught, said and logged

The interface review of 2026-10-30 (`../../review/2026-10-30-ui-review.md` §4.3, finding 21)
found that the three-d scene's two GL passes could fail in two ways and neither was ever
seen. A panic inside `paint_ground` or `paint_glyphs` unwound through `egui_glow`'s paint
callback and ended the process, so the binary's recovery for a poisoned lock, written for
exactly that panic, could never run: nothing survived the frame to observe the poison. A GL
error after a pass was never read, so a draw that failed left a blank region under a status
line that still counted the tracks. The crate had no `tracing` call, so neither left a line
in the log. `rust-ui-architecture-coding-standards.md` §4 asks that a per-frame error be
logged and degrade to a banner, never crash. This item does that, and files the gap it
closes as GAP-259, the last of the review's §4.3 findings without an id.

**The guard.** `gl::guard(pass, body)` runs one pass under `std::panic::catch_unwind` and
turns what comes out into a `GlFault`: `Panicked { pass, what }` for a caught panic, with
the panic's message, or `Gl { pass, error }` when the body's own result is an error. Each
pass now has a private body, `ground_pass` and `glyph_pass`, and its public `paint_*` runs
the body under the guard followed by three-d's `error_check`, which is one `glGetError` and
a framebuffer check per pass per frame. three-d's note is not to call it too often; two a
frame is the price of a GL error being a sentence on screen rather than a blank region, and
is accepted. `AssertUnwindSafe` is what it says: after a caught panic the renderer may hold a
half-finished upload, which is the reason for the next paragraph and not a reason the catch
is unsound.

**The first fault ends the scene for the session.** The renderer keeps the first `GlFault`
and both passes return without drawing once one is held. A renderer that has panicked
halfway through an upload or left OpenGL in an error state is not one to draw from again,
and a scene that came back on the next frame would hide that anything had happened. The
catch runs under the renderer's lock, inside the callback, so the lock is not poisoned; a
poisoned lock can now only mean a panic the guard did not see, and the binary names that
case as `GlFault::Poisoned` rather than reading it as nothing to draw.

**Where it is said.** The callbacks run after the frame and own no `AppState`, so the fault
waits on the renderer and `App::note_gl_fault` reads it under the lock before the next
frame, in the same read `draw_picture` already makes for the ground revision. The first
sight is logged at error level, pushed onto the alert list, and handed to
`ViewportState::note_gl_fault`, which turns `gl_ready` and `use_3d` off and keeps one
sentence: "The three-d scene stopped and is not drawn again this session: the glyph pass
left OpenGL in error: Out of memory. The projection is drawn instead." The projection's
status line gains a variant saying the scene stopped after a fault and that the line below
says what happened; that line is the sentence, in the alert colour, under the status line
for the rest of the session. With `gl_ready` off the corner control is not drawn, so
nothing offers a way back to a renderer that is not drawing. The sentence is formatted once,
at the fault; the status line's key gains a `faulted` flag so it is rewritten once too.

**What is tested, and what is not.** Without GL: `guard` passes a clean pass through, keeps
an error as `Gl` naming the pass, and catches a panic with its message, including a
formatted one; `note_gl_fault` turns both flags off, and a frame of the projection after it
says the scene stopped, draws the fault's sentence in the alert colour, keeps the first fault
over a second, and draws no `2D` or `3D` control. The GL passes themselves are unseen by any
test, as `gl.rs` says of itself; what changed inside them is a private body under a guard,
not the draw. The owner's checks for the scene are unchanged
(`the-owner-s-look-at-the-three-d.md`, `track-glyphs-over-the-three-d-scene-looked.md`,
`gap-252-terrain-in-the-three-d-scene.md`).

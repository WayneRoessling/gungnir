# Track glyphs over the three-d scene, looked at by the owner

GAP-251 put each track's frame, sigma outline, heading vector, number and assignment over the
three-d scene's discs (`the-three-d-scene-draws-each-track-s.md`). That item listed nine
checks for the owner to make on a display, because no test can see the GL draw under the
overlay. The owner looked on 2026-10-30 and reported all nine passed, with check 9 settled
by a capture rather than by eye. Taken as passed on the owner's instruction.

**The session.** The desktop built from main after #269, on the owner's Windows
workstation, against the round-1 baseline and its rehearsal seed, as in the first look
(`the-owner-s-look-at-the-three-d.md`).

**Checks 1 to 8 passed on the owner's display**: the numbers, the frames, the heading lines,
the sigma outlines when zoomed in, the `R1` assignment label beside track 56 with the status
line counting one assignment, the stale track's colour change, the glyphs staying on their
discs while panning and zooming, and nothing drawn over a panel.

**Check 9, whether the discs still draw under the frames, could not be read by eye.** The
owner's screenshots showed the glyphs as frames with apparently empty fills. Every track in
the seed is unknown, so every frame is four lobes. At the 2D glyph's size the four lobes
cover almost all of the disc under them: the disc shows only through the small openings
inside each lobe, and the owner's screenshots were too downscaled to show those.

So the binary the owner was running was started a second time, with the same rehearsal seed,
from a scratch folder with a journal of its own. Its window was captured at native
resolution in the scene, then again after switching to the projection with the corner
control. In the scene the openings inside each lobe are the confirmed track colour; in the
projection the same openings are the viewport background, with only the small lifecycle fill
at the centre. The colour inside the lobes is the GL disc, so the scene still draws under the
overlay. The captures stayed on the workstation and are not part of this record.

**What the look found besides.** An assignment label can overlap a neighbouring track's
number: with tracks 55 and 54 close together, `R1` under track 55 was drawn on the `54`
above track 54. It does so in both renderers, because the label's place comes from
`tracks::draw_plan_2d`, which predates GAP-251.

**What the look does not establish.** It is one look, on one machine, at one classification.
A hostile, friendly or neutral frame covers less of the disc than the four lobes do, and was
not on screen. A change to `gungnir-viewport3d/src/gl.rs` is unseen until somebody looks
again.

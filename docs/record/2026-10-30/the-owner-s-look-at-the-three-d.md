# The owner's look at the three-d scene

The three-d scene had been the desktop's default renderer since 2026-09-05 without anybody
seeing it draw, because no headless probe exists for a GL draw call (GAP-226). The owner
chose to look rather than default it off (D-153). The camera, the clip box and the glyph size
were fixed first, in
`gap-226-the-three-d-scene-readied-for.md`. The owner looked on 2026-10-30 and reported every
check below passed.

**The session.** The desktop on the owner's Windows workstation, built with those fixes,
against the usability round-1 baseline and its rehearsal seed:

```
GUNGNIR_CONFIG=testdata/usability/round-1.json
gungnir-app --rehearsal testdata/usability/round-1-seed.json
```

That gives two radars, a no-go geofence, an approach, a harbour boom and nine scripted tracks
arriving over the first 75 seconds, one of which (39) goes stale at 65 seconds.

**The checks, all passed:**

1. The scene attached: the log line `three-d scene attached to the OpenGL context`.
2. The session started in the scene: the status line began `three-d scene:` and the corner
   control read 3D.
3. The tracks appeared as flat filled discs the 2D glyph's size, arriving on the seed's
   schedule and moving west.
4. Switched with the corner control, each disc sat where the 2D glyph sat against the metre
   grid.
5. Panned and zoomed in the scene, the discs stayed on their grid points and kept their size
   on screen.
6. The colours matched the projection's, and track 39 changed colour on going stale in both.
7. With the window resized and the panels moved, no part of the viewport lost its tracks and
   nothing was drawn over a panel.
8. Display scaling at 100 % and at 150 %.

The owner's screenshots show the same view in both renderers, half a minute apart. They show
the geofence, the approach's gap line and the boom in identical places. In the scene, each
disc sits at the leading end of its predicted path, which egui draws from the same track
positions, and track 39 is drawn in the stale colour.

**What the look does not establish.** It is one look, on one machine. Nothing tests this
path, and a change to `gungnir-viewport3d/src/gl.rs` is unseen until somebody looks again, at
these checks. The scene still draws only the discs: no track number, heading line,
uncertainty outline or assignment line. Those are drawn only by the projection.
GAP-226's other half, the track table drawing every row, is untouched.

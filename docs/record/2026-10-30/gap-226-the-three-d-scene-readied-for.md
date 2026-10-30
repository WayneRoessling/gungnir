# GAP-226: the three-d scene, readied for the owner's look

GAP-226's action was to default the three-d scene off until somebody had seen it on a
display. The owner chose the other way (D-153): the scene stays the default, as decided on
2026-09-05, and the owner looks at it. Before that look was worth having, the draw path was
read end to end against the projection egui draws over it and against three-d 0.19's own
source. Three defects were found. Each would have made the look fail for a reason that was
not the renderer, so each was fixed first.

**The camera showed the wrong amount of ground.** It was a perspective camera with a
45 degree vertical field of view, placed at the height where that field covered the
viewport's *width* in metres, counted in *physical pixels*. The 2D view's scale is in egui
points, and a vertical field covers the height. So the scene showed the overlay's ground
multiplied by the viewport's aspect ratio and by the display's scaling: on a 16:9 picture at
150 %, about 2.7 times too much. Every track would have been drawn toward the centre, away
from the rings, dead spots and bearing rays egui places with `TopDownView::project`. A
track at altitude would also have drifted outward, since perspective enlarges what is
nearer the camera and the projection ignores altitude. The camera is now orthographic and
sized in points. `the_camera_puts_a_track_where_the_projection_does` holds it to the
projection at two zooms, at wide and tall aspects, at 150 % and 200 % scaling, and for a
track 9 km up. three-d multiplies an orthographic height by the camera's distance from its
target, which its documentation states and `camera` allows for.

**three-d clipped the draw to the wrong place.** `RenderTarget::render` clips to a box at
the window's bottom-left corner the size of the target. The viewport is a region inside the
window with panels docked around it, so that box and the viewport only partly overlap. Part
of the picture would have shown tracks and the rest none, with no error. The draw now clips
to the rectangle egui clips the callback to.

**A glyph was 120 m across whatever the zoom.** At sector scale that is a dot of a pixel or
two, and close in it fills the screen. The sphere is now the 2D glyph's radius in points at
the current zoom.

Two smaller changes came with them. The scene now has the projection's background and
metre grid, so the two renderers differ only in how a track is drawn. The grid is also a
fixed reference to judge the scene against. And two comments said the status line was
painted over the scene. egui draws it first and the scene draws after it without clearing,
which is what keeps the line visible, so the comments now say that.

The review's other finding about the scene, two comments disagreeing about which renderer
is the default, had already been resolved: both say the scene. So had the information
architecture's sentence, which says the tables are not virtualised. What remains of GAP-226
is the owner's look and the track table.

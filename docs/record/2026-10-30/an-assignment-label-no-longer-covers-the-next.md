# An assignment label no longer covers the next track's number

The owner's look at the track glyphs (`track-glyphs-over-the-three-d-scene-looked.md`) found
`R1`, the assignment under track 55, drawn on the `54` of the track below it. That is
GAP-253. It happened in both renderers, because both draw the plan through
`tracks::draw_plan_2d`.

**Why it collided.** Each track had two lines of text: its id above and to the right of the
glyph, and its assignment below and to the right. The assignment line of one track occupies
the band where the id line of a track 10 to 38 points below it sits, at the small font's
12-point size. Tracks on a common approach, like the seed's, are often that close.

**The change.** The assignment now follows the id on the id's line, after a 4-point gap, so
each track has one line of text. Two tracks' labels now meet only when the tracks are less
than one line's height apart. The id's width is measured with the font the palette names,
not counted in characters. A track with two resources lists both on that line (`R1 R4`),
where before two labels were drawn at one place, over each other. The place the id line
starts is one function, `id_label_anchor`, used by both the glyph and the plan.

**The tests**, in `gungnir-viewport3d/src/tracks.rs`, read egui's shape list:

- `an_assignment_does_not_cover_a_neighbouring_tracks_number`: three tracks 20 points apart,
  two of them assigned; no painted text overlaps any other.
- `a_tracks_assignments_follow_its_number_on_one_line`: a track with two resources has
  `R1 R4` on its id's line, after the id.

Both fail with the label put back below the glyph; that was tried.

**Not looked at.** The new place is what the tests check; nobody has seen it on a display.
The overlay test from GAP-251 still holds the scene's labels to the projection's, so the two
renderers move together.

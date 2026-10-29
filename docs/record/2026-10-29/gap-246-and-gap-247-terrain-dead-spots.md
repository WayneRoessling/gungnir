# GAP-246 and GAP-247: terrain dead spots on the map

The owner asked on 2026-10-29 for the places the terrain blinds the sensors to an
approaching UAS or aircraft, shown on the 3D map as a darkened area. DN-34
(`../../design/DN-34-sensor-dead-spots.md`) designed it that morning, the owner took its
three decisions to their defaults (`the-owner-s-walk-of-2026-10-29.md`), and this change
builds it. What the build changed against the note is the note's amendment 1; this item
says why the order and the shape of the work were what they were.

## GAP-247 first

The approach report masked line of sight by scanning every DEM vertex for every sample of
every ray, several times a frame. That was harmless on the test grids and a stall on any
real DEM, and the dead-spot field needed the same lookup a million times over, so it was
closed before anything else:

- `gungnir-analytics::terrain::MeshSurface` is `gungnir-app/src/ground.rs`'s triangle
  lookup moved down, so the radar feeds' ground model and the analytics read one surface.
- `HeightField` resamples the placed mesh once per load, its cells on the DEM's vertices;
  `HeightFieldLineOfSight` samples a segment against it every half cell.
- The approach report and PN-16's per-laydown reports read it through a cache keyed on
  their inputs, since the desktop asks for the same report several times a frame.

**Every standing gap acceptance re-opens once on a deployment that loads a DEM.** The
masked gaps move by up to a sample where a ray grazes a ridge, because the lookup changed
from the nearest vertex to an interpolated surface, and DN-33 §5 makes a moved end a shape
change. PN-11 lists each one as a shape change with what is there now. It does not say
the lookup caused it, because the desktop cannot tell that cause from any other; this
item is where that is said, for anyone reading a re-opened acceptance after the upgrade.

## The field, and the error the sweep had to admit

`gungnir-analytics::shadow` computes the field: cells over the active sensors' reach, each
a probe at the stated height above the ground, dark where a sensor claims it and none sees
it (D-146). The sampled reference is the definition; the radial horizon sweep is the
production path. The first sweep disagreed with the reference on one cell of a ridged test
terrain, away from any horizon. The cause was not a defect in either. The ray deciding the
cell ran nine metres beside the true line where that line crossed a steep ridge, and the
probe cleared the ridge by ten. A sweep cannot avoid reading ground slightly beside the line
it stands for. It can know how far off it may be. So each verdict carries a bound built
from the field's steepest slope and the ray's offset, and the reference decides any cell
inside it. On the test terrains the two now agree on every cell, and the sweep keeps most
of its speed.

## On the desktop, off the tick

`gungnir-app::dead_spots` snapshots what the field depends on and hands a change to a
worker thread, drawing the previous field faded and saying "recomputing" until the new one
lands. It computes nothing on the node (D-148) and journals nothing. The viewport draws the
field as one texture through the overlay both renderers share, so it is on the three-d
scene in the same change. PN-11 has the toggle, the height (D-147's one default height),
the choice of one sensor's shadow, the measure, its age and a recompute control. With no
DEM the map says the dead spots are unknown, never clear.

## What was measured and what was not decided

- Over a 1 000 by 1 000 DEM at 30 m with three radars, the field is on the map 363 ms after
  the change, against MOP-14's 10 s, and no tick in between took more than 105 µs. This is
  printed by `gungnir-app/tests/frame_budgets.rs` and asserted nowhere: promoting a budget
  to a gate is the owner's, as every row in that file was.
- DN-34 §11's six verification rows were written as tests and are not added to
  `../../verification-capability-table.md`; a pass criterion is human-owned.
- PN-16's per-option shadow column is GAP-248, the one item of DN-34 §7 not built.
- A draped shadow on a tilted terrain surface is DN-34 §13 increment 2, behind GAP-226.

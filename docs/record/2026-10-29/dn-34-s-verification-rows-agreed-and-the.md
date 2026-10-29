# DN-34's verification rows agreed, and the line of sight made exact

GAP-246's build (`gap-246-and-gap-247-terrain-dead-spots.md`) left DN-34 §11's six rows as
drafts and printed the MOP-14 recompute figure without asserting it, because a pass
criterion and a budget gate are the owner's. The owner read the build's summary the same
evening and approved both: the six rows into the verification table, and the budget as an
assertion. This change carries that out. The signature is in `../../signatures.md`, at the
commit on main whose DN-34 holds the rows as they were agreed.

## Agreeing a row meant testing all of it

The first row, as drafted, names a wall, a cone and a valley, each against its closed-form
horizon. The build had written the wall alone. A row cannot gate a clause nothing checks,
so the cone and the valley were written before the row went into the table. The valley
passed at once, at both ends of its shadow.

The cone did not: its shadow began 54 m beyond the line over its apex, so the field called
ground watched that the apex hides. Two causes were found, one in the test and one in the
code.

- **The test's.** A field's cells are laid out from its sensors' reach, and with the range
  first chosen the row of cells the test read lay half a cell north of the apex line. On a
  cone that steep that is five metres of ground. The wall and the valley do not change
  across the line, so they could not show it. The range now puts a row of cell centres on
  the line.
- **The code's.** The line of sight sampled a ray every half cell, and a sharp peak between
  two samples is read low. That is the unsafe direction for this layer, so the line of sight
  no longer samples. The interpolated surface bends only where a ray crosses a line through
  cell centres, and between two crossings its height is a parabola. The test checks each
  crossing, both ends and each piece's top, and it is exact. The sweep reads the same
  crossings into its horizon.

## The figure the owner read changed

The owner approved on a recompute of 363 ms. With the exact line of sight the same fixture
takes 645 ms on the same machine, because a ray now reads more points. MOP-14's budget is
10 s. The test asserts it in the release profile, with the row's other half: the tick that
sees a change returns with the old field still drawn while the worker computes.

## The register

GAP-246's closing history entry, merged with the build, says the rows stayed drafts. It is
not edited; a later entry says they were agreed.

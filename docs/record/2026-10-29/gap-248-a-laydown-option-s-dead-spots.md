# GAP-248: a laydown option's dead spots on PN-16

GAP-246 built the dead-spot layer for the live picture and left one item of DN-34 §7 out:
item 5, a planner seeing each laydown option's dead spots on PN-16. It was filed as
GAP-248, and the owner asked for it the same evening. DN-34 amendment 3 says what was
built; this item says why it is shaped as it is.

**Computed on selection, one option at a time.** Every option's field is a full
recompute over its sensors' reach, which item 5 already weighed: a table that computed
every row would cost every option's recompute whenever anything changed, for rows nobody
is reading. So selecting an option on PN-16, the same selection that previews its
placements on the map (GAP-087), hands its inputs to the dead-spot worker. Clearing the
selection drops the field. The other rows say "select to compute", which is not the same
claim as an empty column.

**From the option's placements, never the registry.** An option's dead spots are about
where it would put its sensors, so its inputs are the searching and tracking sensors it
places, from `laydown_coverage_volumes`. That is the function its coverage column already
reads, so the two cells in one row are about the same placements. The test makes the
distinction visible. The live radar is at standby, so the live field is empty, while
the option's field has dead spots from the radar the option places.

**What it leaves out, deliberately.** The map keeps showing the live field, because two
fields drawn at once would be two answers under one toggle. There is no difference from the
current laydown's dead spots either, since that would need the current option's field
computed alongside, which is the cost item 5 avoided. A planner compares by selecting each
option in turn and reading its row.

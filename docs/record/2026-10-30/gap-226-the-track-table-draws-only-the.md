# GAP-226: the track table draws only the rows in view

GAP-226's second half was PN-03. It drew every track in an `egui::Grid`, laid out every
frame, while the information architecture called it virtualised and the 8 ms egui gate
timed a workspace with no table in it. WF-03 asks for a virtualised table, sortable by any
column and filtered by class and status. This is that, and the gate now covers it.

**Why the grid had to go, not just be wrapped.** `ScrollArea::show_rows` lays out only the
rows in view, but a grid sizes its columns from the rows it lays out, so a virtualised grid
either jitters as rows scroll past or keeps its header out of line with them. Each column
now has a width measured from its heading and from a sample of the widest thing it can
hold: the longest status and class word in the deployment's vocabulary, a six-digit
coordinate, `-00000.0 unknown` for the up column. The header and every row are cells of
exactly those widths, so they line up without the rows being looked at. A value wider
still, such as a long list of parties in the marking, is cut short with the whole of it on
hover. `show_rows` needs one row height, so every cell is made exactly as tall as the
tallest thing a row holds, a selectable id.

**Sort and filter.** Every heading sorts, and a second click reverses. Equal rows keep
their id order in either direction, so they never swap places between frames. A track with
no asset or no score sorts last whichever way the column runs. Class sorts in threat order,
hostile first, and status in lifecycle order. The filter takes a class, a status and
"stale only", all named in the deployment's words. Both the order and the filter are
session state on `AppState`, applied from panel actions like the selection. The table
holds nothing. A locked console may still sort and filter, since that changes only what is
shown (D-143). The count line says how many tracks there are, how many are stale and, when
filtered, how many are shown. A filter that hides every track says so in the warning colour,
because an empty table would otherwise read as an empty picture.

**The gate had been measuring four tracks.** `the_egui_pass_meets_its_budget` and
`snapshot_calls_meet_their_budget` ticked ten seconds of the Scenario 4 replay and then
waited only for the first track. So both budgets, stated "at Scenario 4 track counts", were
timed against the four or so tracks the pipeline had confirmed by then. On this seed the
picture passes 200 tracks about 45 s into the replay and settles near 340 with the swarm's
false tracks. Both tests now drive the replay until the picture holds Scenario 4's 200
targets, and fail if it never does. The egui gate also times the operator's workspace, the
one with the table, and checks that the table is drawn in it.

Release figures, one run each on the owner's workstation (timings on this machine vary by
about two times between core types, so these are indications, not a benchmark):

| Workspace, 212 tracks | Before (grid) | After |
|---|---|---|
| Operator, p99 | 6.86 ms | 0.92 ms |
| Commander, p99 | 1.23 ms | 0.37 ms |
| Operator, worst of 200 | 26.8 ms | 10.1 ms |

The commander's workspace has no table, so its change is the machine's noise, not this
change. Against the 8 ms budget the old table at Scenario 4's count was within reach of
failing at p99, and its worst passes were over three times the budget. The
verification table's `gungnir-ui` row records a first figure of 386 microseconds, read when
the row was gated. That figure was taken against the smaller picture. The criterion is
unchanged and is met at the stated count, but the figure beside it is not one measured
there.

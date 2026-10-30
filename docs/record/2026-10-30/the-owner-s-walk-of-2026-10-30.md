# The owner's walk of 2026-10-30: D-149 to D-152

Reviewing whether PN-10 should hold the dead-spot one-sensor chooser turned up a larger
gap than the chooser's placement. DN-34 §7 item 6 wanted the one-sensor view so a sensor
manager could see what re-aiming or raising a mast would do. Nothing answered that short
of editing the baseline. The owner asked for it researched and resolved. The research
found the parts already built: a sensor's volume in any mode, a field over any set of
sensors, and a worker to compute it off the tick. What remained was four product choices,
put to the owner in one batch with a recommended default each. Two were taken past the
default. Each decision's outcome is in `../../mission/gap-analysis/data/decisions.yaml`,
and GAP-249 carries the history.

| Decision | What the owner chose | Against the default? |
|---|---|---|
| D-149 | A what-if may switch one sensor on whatever its mode, raise or lower its mast, turn its sector, **and move it** | **Yes**: the default kept its position fixed, leaving moves to laydown options |
| D-150 | The map shows the live dead spots with what the change would clear and darken marked over them | No |
| D-151 | **Each what-if is recorded on the journal with who ran it** | **Yes**: the default was unrecorded, like the field itself |
| D-152 | The controls live on PN-11, beside the chooser | No |

## What the two departures changed

- **D-149 adds a move**, so a what-if and a laydown option overlap for one sensor moved.
  They still differ in kind: a what-if is one sensor, from the live picture, tried and
  cleared without touching the baseline. A laydown is a whole placement the baseline
  declares, compared against the others.
- **D-151 makes a what-if a record**, with three consequences the build draws, set out in
  DN-34 amendment 4 item 3. A run needs somebody signed in. It is recorded once, when its
  first answer arrives. And it is a planning-record event, not an audited action, because
  it changes nothing.

## What the walk did not decide

- Several sensors changed at once. A laydown option already does that, and nothing here
  asks for a second way.
- Turning a what-if into a task for the sensor. That would put a command on the planning
  surface, which is the line DN-26 drew for laydowns too.

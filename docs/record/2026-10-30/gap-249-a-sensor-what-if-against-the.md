# GAP-249: a sensor what-if against the dead spots

GAP-249 is built as DN-34 amendment 4 describes, under the owner's decisions of the same
day (`the-owner-s-walk-of-2026-10-30.md`). This item says why it is built the way it is.

**Comparing two fields meant giving them one grid.** A field's cells are laid out from its
sensors' reach. A sensor switched on, moved or given a longer view changes that reach, so
the live field and the field with the change would land on different grids, and no cell of
one would be a cell of the other. `shadow_field_over` lays a field over an extent it is
given. The what-if passes both sets' reach together, at one cell, and `compare_fields`
refuses two fields that are not one grid rather than comparing them loosely.

**What "would go dark" means follows D-146.** A dead spot is a place a sensor claims and the
terrain hides, so switching on a standby radar behind a ridge adds dead spots. It adds
claims the terrain denies, and the map marks them newly dark. The same run also raises the
watched area, and PN-11 says both, so the operator reads the trade rather than one side of
it. The test shows exactly that. Switching the ridge test's standby radar on darkens the
valley behind the ridge and clears nothing; raising its mast to 300 m clears the valley.

**Recorded, but not an act.** The owner chose to record every what-if (D-151). The record
is a planning event, next to a gap acceptance and a laydown rehearsal, because like them
it is something a person did at the planning surface. Unlike a gap acceptance it gates
nothing. So it needs somebody signed in, there being nobody to record otherwise, but no
particular role. It writes no audit entry, since the audit log is for gated actions. Each
run is recorded once, with the first answer it found. A run cleared before it has an answer
leaves no record. The same run recomputed after a live change is not recorded again,
because recording it again would read as somebody asking twice.

# The egui gate's figure, re-read at Scenario 4's count

The `gungnir-ui` row of `../../verification-capability-table.md` gates the egui pass at
p99 under 8 ms at Scenario 4 track counts. It was gated on 2026-09-16 after the owner read a
first figure of 386 microseconds. GAP-226 found that the test behind that figure stopped
waiting at the first track, so the figure was measured on a picture of about four tracks
(`gap-226-the-track-table-draws-only-the.md`). The test now drives the replay to Scenario 4's
200 targets and times the operator's workspace as well as the commander's.

On 2026-10-30 the owner asked for the row to say so. The row keeps the 386 microsecond
figure as what was read at gating, says what it was measured on, and adds the figures
measured at the stated count: p99 0.92 ms for the operator's workspace and 0.37 ms for the
commander's, at 212 tracks, one release run each.

**The criterion is unchanged.** It still reads "egui pass p99 under 8 ms at Scenario 4
track counts", and the test asserts it in release as before. The edit corrects the evidence
beside the criterion; it does not touch the criterion itself.

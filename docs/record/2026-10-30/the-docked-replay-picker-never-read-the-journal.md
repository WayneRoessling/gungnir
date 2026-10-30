# The docked replay picker never read the journal's sessions

The 2026-10-30 UI review (`docs/review/2026-10-30-ui-review.md` §4.2, finding 6) found that
PN-12's session picker, when docked, listed nothing for the whole session, over a journal
that had sessions in it. That is GAP-254, filed and closed in this change.

**Why it was empty.** The journal directory was read once, when `sessions_read` was false,
in `App::draw_docked` in `gungnir-app/src/main.rs`. Only `draw_detached_panel` reached that
function. The dock tree draws its panes through `PanelBehavior::draw_panel` in
`gungnir-app/src/dock.rs`, whose `Replay` arm passed `sustainment.sessions` to the panel
without ever filling it. The analyst, intelligence analyst, planner and administrator
layouts all dock PN-12 (`docs/ux/information-architecture.md` §4), so for each of them the
picker said "No sessions recorded." until the baseline detached the panel. The two draw
paths had been written as copies of each other when the tree arrived (GAP-075), and the
read was added to one copy.

**Why no test saw it.** The headless tests in `gungnir-app/tests/rendered_workspace.rs` drew
PN-12 over an untouched desktop. The journal creates a session's file on its first append,
so that desktop has no session on disk, and "No sessions recorded." was the truth there. The
project's rule that an empty list says why it is empty was met by the words and defeated by
the path: a list empty because nothing read it says the same thing as a list empty because
there is nothing to read.

**The change.** `SustainmentState::read_sessions_once` in `gungnir-app/src/sustainment.rs`
is now the one place the sessions are read: if `sessions_read` is false it calls
`session_summaries` and sets the flag. Both `PanelBehavior::draw_panel` and `App::draw_docked`
call it, so the two paths cannot drift apart again. The read is still taken once when the
picker appears and not every frame, and `ReplayAction::Close` still clears the flag so a
reopened picker re-reads the lengths.

**The test**, `the_docked_replay_picker_lists_the_journals_sessions`, publishes one event,
ticks, saves the session, and draws the analyst's real dock tree (`dock::default_arrangement`,
`dock::tree_from`, the real `PanelBehavior`) through the render probe. It asserts the
picker's heading is drawn, that "No sessions recorded." is not, that a text equal to
`Session <id>` is, that no length reads as unread, and that `sessions_read` is set after
the frame. The equality is whole-text on purpose: PN-13 shares the tree and its heading
begins "Session 1. Every figure below...", so a substring match would have passed on the
wrong panel. The test fails with the read removed from `draw_panel`; that was run.

**Looked at and left.** `session_summaries` counts a session's envelopes by calling
`read_session`, which opens the file, reads every line and parses each one into an
`Envelope`. The cost of showing the picker is therefore the whole journal, read and parsed,
once per appearance and again after each replay closes. On this machine's test journals it is
not noticeable; on a long-running node's journal of many large sessions it will be, and the
honest fix is a length that does not need the envelopes decoded, which is `gungnir-store`'s
business and not this change's. Nothing was measured on a large journal.

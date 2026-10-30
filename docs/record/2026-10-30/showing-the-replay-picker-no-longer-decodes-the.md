# Showing the replay picker no longer decodes the whole journal

GAP-254's record (`the-docked-replay-picker-never-read-the-journal.md`) noted that
`session_summaries` learned each session's length by calling `read_session`, which opens the
file, unseals every line and parses every envelope, and left it. That is GAP-256, filed and
closed in this change.

**The cost, measured.** A journal of ten sessions of 20,000 track-update envelopes each,
125 MiB on disk, written and read in release on this machine:

| Lengths of all ten sessions by | Round 1 | Round 2 | Round 3 |
|---|---|---|---|
| `read_session(id).len()` | 449 ms | 430 ms | 436 ms |
| `session_len(id)` | 71 ms | 67 ms | 71 ms |

The picker takes those lengths when it first appears and again after each replay closes,
on the frame, so the first figure is a stall of near half a second on a journal that size.
Both rows depend on which of this machine's two core classes the thread landed on, so the
ratio is the thing to read, not the milliseconds. The harness
was a temporary test, run once and deleted; nothing in the suite times this.

**The change.** `EventJournal::session_len` is a new trait method with a default that
decodes everything, so any implementation is correct before it is fast. `FileEventJournal`
overrides it: it flushes the open buffer as `read_session` does, counts the file's non-empty
lines, and decodes only the last one. The body of a file is trusted to decode because
`append` refuses any line that would not read back (GAP-126); the last line is the one that
can be torn by a process dying mid-append, and `read_session` drops that line, so the count
decodes it and drops it under the same rule. A sealing failure on that line is returned as
the error it is, never counted as torn, which is the mistake GAP-060's review found in
`read_session` once and this method must not repeat. A last line that is not UTF-8 fails the
count as it fails the read. The flush-then-check-the-file-exists prologue the two methods
share is now `path_for_reading`.

`session_summaries` takes the length from `session_len`. The picker's words are unchanged:
a session whose length cannot be read still says "length not read", not zero.

**The tests**, in `gungnir-store/src/lib.rs`:

- `session_len_is_what_read_session_would_return`: five buffered, unflushed envelopes count
  as five; a torn line appended by hand to the file counts for neither; a session with no
  file is `UnknownSession`, not zero.
- `session_len_without_the_key_is_an_error_not_a_shorter_count`: a journal sealed with a
  reversing sealer, read back by a journal with no sealer, fails both the read and the
  length as `Sealing`.

**Not done.** The remaining 70 ms is a read of 125 MiB through the page cache, allocated a
line at a time. Counting newlines in blocks would shave it further, at the cost of a
second rule for which lines are empty; nothing on screen asked for that yet.

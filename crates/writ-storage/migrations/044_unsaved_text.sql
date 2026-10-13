-- Text a note's save could not write (ADR-044).
--
-- The file is the only copy of a note's text (ADR-028 section 1), and this is
-- the one exception it allows: text that has not reached a file.
--
-- in_tab is 1 for the text of the tab that typed it, from the save that could
-- not land to the next save of that tab that does. A note has at most one such
-- row. It turns 0 when the note's tab stops being that tab: the note reopened
-- from history, or the app launched again. A row at 0 is no save's to clear,
-- and a launch puts its text on disk.
--
-- path is the note's file when the row was written, for the log; the launch
-- writes to the file the note's row names then. recorded_at is RFC 3339.
-- The rows go with their note's row.

CREATE TABLE unsaved_text (
    id          INTEGER PRIMARY KEY,
    buffer_id   TEXT NOT NULL REFERENCES buffers(id) ON DELETE CASCADE,
    path        TEXT,
    text        TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    in_tab      INTEGER NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX unsaved_text_in_tab ON unsaved_text (buffer_id) WHERE in_tab = 1;

CREATE INDEX unsaved_text_note ON unsaved_text (buffer_id);

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
-- path is the note's file when the row was written, for the log and for text
-- whose note's row has since been deleted; the launch writes to the file the
-- note's row names then. text_hash is the SHA-256 of text, which a record
-- compares instead of reading the text back. recorded_at is RFC 3339.
--
-- buffer_id names the note's row and has no foreign key: a row keeps its own
-- lifetime and outlives the note's row, until a launch puts its text on disk
-- or the person discards it.

CREATE TABLE unsaved_text (
    id          INTEGER PRIMARY KEY,
    buffer_id   TEXT NOT NULL,
    path        TEXT,
    text        TEXT NOT NULL,
    text_hash   TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    in_tab      INTEGER NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX unsaved_text_in_tab ON unsaved_text (buffer_id) WHERE in_tab = 1;

CREATE INDEX unsaved_text_note ON unsaved_text (buffer_id);

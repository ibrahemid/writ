-- The folded name each link names: the key a note answers to when a link
-- names it (writ_core::notes::links::candidate_name_keys), read off the
-- stored target with the same rule.
--
-- A note that arrives or leaves changes only the links that name it. With the
-- key on the row, finding them is one indexed lookup; without it, every save
-- and every delete read every link in the folder to find the few that named
-- the note.
--
-- The column is filled for the rows already here inside this migration's
-- transaction, from to_target, by the migration runner: the fold is Unicode
-- normalisation, which SQL does not have. No note is read.

ALTER TABLE links ADD COLUMN name_key TEXT;
CREATE INDEX idx_links_name_key ON links(name_key);

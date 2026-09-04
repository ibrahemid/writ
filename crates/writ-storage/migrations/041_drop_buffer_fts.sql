-- The index is keyed by canonical path now (ADR-028 section 7). buffer_fts
-- joined buffers on rowid and indexed mirror text that no longer exists. The
-- replacement, files_fts, was created empty by migration 040 and is populated
-- by the reconcile walk, so nothing here needs to move data.

DROP TABLE buffer_fts;

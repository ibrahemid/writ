-- Rebuild the FTS index with a prefix-token index and the unicode61
-- tokenizer (diacritic-folding). The original buffer_fts (migration 001)
-- has no prefix index, so a 2-4 character prefix query (`"tok"*`) scans the
-- whole term list instead of seeking — measured at 14 ms median over 10k
-- buffers in the 2026-05-22 audit. `prefix='2 3 4'` builds auxiliary indexes
-- for 2-, 3-, and 4-character prefixes, turning that scan into a seek.
--
-- The rebuild copies rowid verbatim: buffer_fts joins buffers on rowid, so a
-- copy that dropped rowid would silently break every search.
CREATE VIRTUAL TABLE buffer_fts_v2 USING fts5(
    title,
    content,
    prefix='2 3 4',
    tokenize='unicode61 remove_diacritics 2'
);

INSERT INTO buffer_fts_v2(rowid, title, content)
    SELECT rowid, title, content FROM buffer_fts;

DROP TABLE buffer_fts;

ALTER TABLE buffer_fts_v2 RENAME TO buffer_fts;

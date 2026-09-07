-- Same tokenizer and prefix set as buffer_fts after migration 030, so search
-- behaviour does not change when the index is re-keyed to paths.
CREATE VIRTUAL TABLE files_fts USING fts5(
    name,
    content,
    prefix='2 3 4',
    tokenize='unicode61 remove_diacritics 2'
);

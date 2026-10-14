-- The folded search index. Holds the notes whose name or text carries Han,
-- kana, Hangul or Arabic characters, folded by
-- writ_core::search::fold_for_search: NFC, Arabic harakat, Quranic marks and
-- tatweel dropped, alef, ta marbuta and alef maqsura variants read as one
-- letter. The trigram tokenizer finds a word inside a run that unicode61
-- reads as one word ("写作" in "我喜欢写作"), and a query is folded the same
-- way before it is asked.
--
-- Joined to files on rowid, like files_fts. The rows already indexed are
-- filled from files_fts by migrate_rows, so no note is read again.
CREATE VIRTUAL TABLE files_folded USING fts5(
    name,
    content,
    tokenize='trigram remove_diacritics 1'
);

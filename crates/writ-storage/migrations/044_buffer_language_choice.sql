-- The language column now holds only a language the person chose for a file
-- with Change Language; NULL lets the editor decide from the file's name.
--
-- Until now the open path filled it from the file's extension, so every
-- existing value is a guess from a name that may since have changed, and some
-- of them name grammars the editor no longer picks for that extension. Kept,
-- they would read as choices and pin each file to the old guess.

UPDATE buffers SET language = NULL;

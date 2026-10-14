-- The text encoding the note's file is in (ADR-049), as the encoding's name
-- followed by ' BOM' when the file starts with its byte order mark:
-- 'UTF-8', 'UTF-16LE BOM', 'windows-1252', 'Shift_JIS'.
--
-- The editor works in UTF-8, so a save encodes back into this. NULL for a row
-- whose file is not read as text: a binary file's hex view and a document Writ
-- generated. Every text row before this one was read as UTF-8, which is what
-- it records.

ALTER TABLE buffers ADD COLUMN encoding TEXT;
UPDATE buffers SET encoding = 'UTF-8' WHERE read_only = 0;

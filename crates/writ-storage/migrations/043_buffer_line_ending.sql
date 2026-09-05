-- Which line ending the note's file uses: 'lf' or 'crlf'.
--
-- CodeMirror hands its document back with '\n' breaks whatever the file held,
-- so without this every save rewrites a Windows note line by line. The ending
-- is read once when the file is first read and re-applied on the way out, and
-- it has to survive a restart or the first save after relaunch normalises the
-- file anyway.
--
-- Every existing row predates the detection, so the default is LF, which is
-- also what a note Writ creates gets.

ALTER TABLE buffers ADD COLUMN line_ending TEXT NOT NULL DEFAULT 'lf';

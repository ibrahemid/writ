-- Per-document opt-in to run the document's own scripts in the preview
-- (decision RC-D1/TS-D1). Scripts are off for every document by default; a
-- file the user opts in is remembered here, on its buffer row, so the choice
-- survives closing the tab and relaunching.
--
-- 0 = off (the default for every row, new and existing), 1 = on. A history
-- buffer reopened by its source path keeps the flag, which is what "remembered
-- per file" means for a saved file.

ALTER TABLE buffers ADD COLUMN preview_scripts INTEGER NOT NULL DEFAULT 0;

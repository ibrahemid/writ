# ADR-048: Config is read leniently, written atomically and holds no derived data

## Status

Accepted, 2026-10-14.

## Context

One bad value in `config.toml` stopped Writ from starting, with advice about the data folder that did not apply. Every palette command rewrote the whole file to bump a use count, and the frontend held the config as one signal, so each run reran every reader of every setting, including the AI connection's model-list request. `storage.path` was parsed and never read.

## Decision

1. **Lenient at startup.** `WritConfig::parse_lenient` keeps every value that reads. Each value that does not is found by reading it on its own, removed, and named as a diagnostic `{key, line, message}`; it falls back to what a file without it reads as. A file that is not TOML runs on `WritConfig::default()` with one diagnostic at the parser's line. Reloads stay strict: a hand edit that breaks the file keeps the last good config.
2. **Nothing is written while values are unread.** The diagnostics live on `AppState`. While any are held, `persist_config` changes the running copy and skips the disk write, so the defaults standing in for the bad values never replace the person's file. A reload that reads the file in full clears them. The frontend shows a notice naming the first key and line until then. Moving the notes folder is the one write made while they are held: its files have already moved, so `NotesConfig::write_root_into` sets `notes.root` in the file's text and leaves every other byte as it was. A file that is not TOML, or holds a plain value at `notes`, turns the move down before anything moves.
3. **Atomic, serialised writes.** The file is replaced through a temp file and a rename (`ConfigStore::write_serialized`). `mutate_config` holds the config lock from the copy to the swap, so two changes made through it never lose one another; `update_config`, the first-run writes and the spelling list use it, and the other commands that write the config move to it as they are touched.
4. **No derived data.** Command usage lives in the `command_usage` table of `writ.db` (migration 049) behind `get_command_usage`, `record_command_uses`, `clear_command_usage` and `prune_command_usage`. The first launch with the table moves a `[commands.usage]` table it finds, once, marked in `schema_meta`. `storage.path` is removed. Both old tables still parse and are dropped on the next write.
5. **Fine-grained frontend.** `configStore` is a `createStore`; writes go through section setters or `reconcile`, so a change reruns only the readers of the fields it changed. Panel open state is read from the config, not copied out of it.

## Consequences

A setting changed while values are unread lasts the session only, apart from the notes folder, and a program's approval written then does not reach the `writ mcp` server, which reads the file. The config watcher follows the folder, so a `config.toml` created after launch is heard.

# ADR-053: The open file, for a program allowed to see it

## Status

Accepted, 2026-10-15. Decision MK-D1. Amends [ADR-031](./031-the-ai-harness-and-what-leaves-the-machine.md) rules 1.1, 1.2 and 1.4; rule 2.4 stands.

Amended 2026-10-15: a program's edits to the open file. Its write reaches the tab as any outside change does ([ADR-033](./033-external-change-handling.md) decision 16), one smallest change under the hash guard. The tab learns the writer from the version store: `history.db` rows gain `writer` (added in place; older rows name none), and the note's newest row holding the bytes names the program. Its inserted text is marked until the person edits or leaves the tab, and Revert To names each version's writer. A program set to Ask first (`approved_clients.review`, read only with writing) has each write held: `writ mcp` builds the bytes, leaves the proposal in `write-reviews/` beside an `OpenRequest` carrying a `ReviewAsk`, and lands it only on Apply from the main window, against the bytes it was built from. Discard, Escape, no app, no ready window, an unreadable answer or five minutes unanswered write nothing; the app brings its window forward to ask. The switch asks for no native confirmation, since it only holds back writes the grant already allows.

## Context

A connected program could read files and nothing else, so "fix this paragraph" took a copy and a paste: it could not see which file was in front in Writ or what was selected there, or point the window at a heading. The selection lives in the window, sometimes unsaved and sometimes in a file outside the notes folder, and `writ mcp` is a separate process with no channel to the app.

## Decision

1. **Two tools.** `get_context` answers the file in front in the main window, its non-empty selections with the line and column each starts and ends at, the caret, and whether the tab holds unsaved text. `reveal` shows a file at a heading or a line; `writ mcp` finds the heading in the file's text (`writ_core::open_context::heading_line`, compared as `[[Note#Heading]]` is) and hands Writ the `writ://open?path=…&line=N` link of ADR-052, so the link grammar is unchanged.
2. **A third grant.** `[mcp] approved_clients` gains `context`, off for every program until the person turns on Open file on its row in Settings, Apps, Connected programs. Reading and writing do not carry it, and it carries neither. Without it `get_context` is refused before the app is asked. `reveal` is a read tool, approved with reading; finding the heading is not a read a later write can lean on.
3. **The channel is a request and an answer file, one pair per call.** When no process holds `instance.lock`, `writ mcp` answers that no file is open and does not start Writ. Otherwise it leaves an `OpenRequest` carrying a `ContextAsk` (a token and the program's name) in `open-requests/`, which the app already watches, and looks for `context-answers/<token>.json` every 25 ms for up to five seconds. The app checks its settings grant that program `context`, emits `context:asked` to the main window only, takes the window's answer through `context_answer` from the window labelled `main`, names the file from its own record of that tab, and writes the answer through `write_new`, readable by its owner only on macOS and Linux. The server reads it and deletes it; answers nobody took are deleted at the app's next start. An ask opens no tab and does not bring the window forward. Nothing listens: the app's side is a filesystem watch it already held, the server looks only while one call waits, and no port or socket is opened at rest or during a call.
4. **Spelling.** A file inside the folder the server serves is named relative to it, with `in_folder` and an `open_url`; any other file by its name alone (AM-20). Selections are held to 100 ranges and 2 MB, cut on a character, with `truncated` set.
5. **Activity.** Every `get_context` call is a line of its own, never folded, naming the file and never the selected text.

## Consequences

- The text answered is the window's and can differ from the file; `unsaved` says so, and a program's write still goes through the guarded write against the file's bytes, so a passage not yet on disk is refused rather than guessed at.
- Turning on Open file asks for no native confirmation, as a grant of reading does not; rule 3.8 covers writing.
- A window not ready yet answers that no file is open.

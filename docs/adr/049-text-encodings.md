# ADR-049: Text encodings

## Status

Accepted, 2026-10-14. Amends [ADR-019](./019-open-anything-ladder.md): UTF-16 is text.

## Context

Every read was `read_to_string`. A Windows-1252, Latin-1, Shift-JIS or GBK file failed to open, UTF-16 opened as a hex view, a UTF-8 byte order mark was dropped on the first save, and the status bar said "UTF-8" whatever the file held.

## Decision

1. **Detect once, at the open** (`writ_core::text_encoding`). A byte order mark, then UTF-16 without one, then the NUL sniff (Binary), then UTF-8, then chardetng's guess. A guess that cannot decode every byte gives way to Windows-1252, which can. ISO-8859-1 is read as Windows-1252, as the WHATWG table does; both map every byte back to itself.
2. **Keep it on the row**: `buffers.encoding` beside `line_ending` (migration 050), the WHATWG name plus ` BOM` when the file has one. `NULL` for a hex view or a generated document, which have no encoding to show; text rows from before the column are UTF-8. The open's `BufferDocument` carries it.
3. **Decode strictly, encode strictly.** A read in the row's encoding that would need a replacement character fails with `ERR_READ_NOT_UTF8` or `ERR_READ_NOT_IN_ENCODING`. A save encodes after the line ending is applied; a character the encoding has no bytes for refuses the save with `StorageError::EncodingLossy`, `ERR_ENCODING_LOSSY: <name> has no bytes for U+…`, and the editor names the characters. Nothing is substituted in either direction.
4. **Bytes and text stay apart.** The write guard, the recorded disk state and the version history take the file's bytes; the editor's comparison digest is taken over the decoded text (`TextEncoding::comparison_digest_hex`). A byte order mark another program adds or removes is followed on the next read.
5. **Reopen with Encoding** (`reopen_with_encoding(buffer_id, encoding)`, the status bar's encoding control and the palette) re-reads the file in the chosen encoding, refuses one the file is not text in, records it, and reloads the tab. A tab with unsaved text is not reopened.
6. **One reader for files without a row.** The notes index the `writ` verbs answer from, the MCP server's `read_note` and text piped to `writ` decode through `decode_file`, the open's detection. MCP hashes stay digests of the file's bytes.

## Consequences

- chardetng can misdetect a short file; Reopen with Encoding is the way out.
- UTF-16 that is mostly CJK has too few zeros for the heuristic and goes to chardetng.
- A Shift_JIS character with two byte forms (the NEC and IBM extensions) is written back in encoding_rs's form.
- MCP `write_note` writes UTF-8 over a file in another encoding; link rewrites and chat edits read UTF-8 files only.

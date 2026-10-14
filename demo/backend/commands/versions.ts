import type { DiffHunk, DiffLine } from "../../../src/services/tauri";
import { VersionMissingError, countUtf8Bytes } from "../history";
import { formatDottedStamp } from "../naming";
import { refuseOnThrow, type Answer, type CommandTable, type DemoState } from "../state";
import { NOTES_ROOT, dirname, extension, stem } from "../vfs";

/** note_history::unreadable. */
function describeUnreadableVersion(error: unknown): string {
  return error instanceof VersionMissingError ? error.message : "That version could not be read.";
}

/** Every line of `before` and `after` as one hunk, matched by their longest
 * common run of lines: the demo's notes are short enough to show whole. */
export function wholeTextDiff(before: string, after: string): DiffHunk[] {
  const a = before.split("\n");
  const b = after.split("\n");
  if (a[a.length - 1] === "") a.pop();
  if (b[b.length - 1] === "") b.pop();
  const common: number[][] = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      common[i][j] = a[i] === b[j] ? common[i + 1][j + 1] + 1 : Math.max(common[i + 1][j], common[i][j + 1]);
    }
  }
  const lines: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      lines.push({ kind: "context", text: a[i] });
      i += 1;
      j += 1;
    } else if (j < b.length && (i === a.length || common[i][j + 1] >= common[i + 1][j])) {
      lines.push({ kind: "added", text: b[j] });
      j += 1;
    } else {
      lines.push({ kind: "removed", text: a[i] });
      i += 1;
    }
  }
  if (lines.every((line) => line.kind === "context")) return [];
  return [{ before_start: a.length > 0 ? 1 : 0, after_start: b.length > 0 ? 1 : 0, lines }];
}

export function createVersionCommands(state: DemoState): CommandTable {
  const { folder, history } = state;
  const readVersion = (args: Record<string, unknown>) =>
    refuseOnThrow(() => history.getEntry(Number(args.versionId)), describeUnreadableVersion);

  return {
    note_versions: (args): Answer<"noteVersions"> => {
      const path = String(args.path);
      const versions = history.listVersions(path);
      const inside = path.startsWith(`${NOTES_ROOT}/`);
      return {
        versions,
        no_versions_reason: versions.length > 0 ? null : inside ? "none_yet" : "outside_folder",
      };
    },
    compare_note_version: async (args): Promise<Answer<"compareNoteVersion">> => {
      const kept = await readVersion(args);
      const current = folder.has(kept.path) ? folder.read(kept.path) : "";
      return wholeTextDiff(current, kept.text);
    },
    note_version_content: (args) =>
      refuseOnThrow(() => history.getEntry(Number(args.versionId)).text, describeUnreadableVersion),
    restore_note_version: async (args): Promise<Answer<"restoreNoteVersion">> => {
      const kept = await readVersion(args);
      const note = kept.path.slice(NOTES_ROOT.length + 1);
      const replacedText = folder.has(kept.path) ? folder.read(kept.path) : null;
      state.writeNote(kept.path, kept.text, "restore");
      await state.emitExternalWrite(kept.path, kept.text);
      const replaced =
        replacedText === null
          ? undefined
          : history.listVersions(kept.path).find((version) => history.getEntry(version.id).text === replacedText);
      return { note, bytes: countUtf8Bytes(kept.text), replaced: replaced?.id ?? null };
    },
    copy_note_version: async (args): Promise<Answer<"copyNoteVersion">> => {
      const kept = await readVersion(args);
      const dir = dirname(kept.path);
      const name = state.findFreeName(dir, `${stem(kept.path)} (recovered ${formatDottedStamp(new Date())})`, extension(kept.path));
      folder.write(`${dir}/${name}`, kept.text);
      state.emitNoteChanged(`${dir}/${name}`);
      return { name };
    },
  };
}

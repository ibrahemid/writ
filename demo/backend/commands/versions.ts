import { VersionMissingError, countUtf8Bytes } from "../history";
import { formatDottedStamp } from "../naming";
import { digestText, refuseOnThrow, type Answer, type CommandTable, type DemoState } from "../state";
import { NOTES_ROOT, dirname, extension, stem } from "../vfs";

/** note_history::unreadable. */
function describeUnreadableVersion(error: unknown): string {
  return error instanceof VersionMissingError ? error.message : "That version could not be read.";
}

export function createVersionCommands(state: DemoState): CommandTable {
  const { folder, history } = state;
  const readVersion = (args: Record<string, unknown>) =>
    refuseOnThrow(() => history.getEntry(Number(args.versionId)), describeUnreadableVersion);

  return {
    note_versions: (args): Answer<"noteVersions"> => history.listVersions(String(args.path)),
    note_version_content: (args) =>
      refuseOnThrow(() => history.getEntry(Number(args.versionId)).text, describeUnreadableVersion),
    restore_note_version: async (args): Promise<Answer<"restoreNoteVersion">> => {
      const kept = await readVersion(args);
      const note = kept.path.slice(NOTES_ROOT.length + 1);
      state.writeNote(kept.path, kept.text, "restore");
      const tab = state.findActiveBuffer(kept.path);
      if (tab) {
        state.bridge.emit("writ://buffer-external", {
          kind: "buffer:external",
          payload: { bufferId: tab.id, path: kept.path, change: "modified", newPath: null, diskHash: await digestText(kept.text) },
        });
      }
      return { note, bytes: countUtf8Bytes(kept.text) };
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

import { Show } from "solid-js";

/**
 * What keeps the notes folder on the user's other machines.
 *
 * Writ syncs nothing itself, so the answer is always a service the user
 * already runs. When the folder is in one, it is named: that is the fact that
 * settles "will this note be on my laptop tonight".
 */
export function NotesSyncNote(props: { provider: string | null }) {
  return (
    <span class="settings-notes-note" data-notes-sync>
      <Show
        when={props.provider}
        fallback="Writ has no sync. Put the notes folder in iCloud Drive, Dropbox, or Google Drive and your notes sync with it. Use one sync service per folder."
      >
        {(provider) => `${provider()} syncs this folder. Use one sync service per folder.`}
      </Show>
    </span>
  );
}

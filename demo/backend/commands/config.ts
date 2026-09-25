import type { WritConfig } from "../../../src/types/config";
import type { IndexStatus } from "../../../src/types/search";
import type { Answer, CommandTable, DemoState } from "../state";
import { HOME, NOTES_ROOT } from "../vfs";

export function createConfigCommands(state: DemoState): CommandTable {
  return {
    // Config and first run
    get_config: () => structuredClone(state.config),
    update_config: (args) => {
      state.config = structuredClone(args.config as WritConfig);
      return null;
    },
    first_run_state: () => ({
      first_run: false,
      hint_dismissed: true,
      file_manager: "Finder",
    }),
    dismiss_first_run_hint: () => null,
    finish_first_run: () => null,
    get_notes_migration_report: () => null,
    dismiss_notes_migration_report: () => null,
    get_storage_info: () => ({ db_path: `${HOME}/.writ/writ.db`, dir: `${HOME}/.writ` }),
    global_hotkey_status: () => ({ chord: state.config.hotkey.toggle, registered: true }),
    cli_status: () => ({ installed: true, path: "/usr/local/bin/writ" }),
    report_first_paint: () => null,
    reveal_window: () => null,
    compute_window_placement: () => null,
    set_caption_button_metrics: () => null,
    confirm_quit_flush: () => null,
    record_unsaved_notes: () => null,
    // The updater is the host's; from a page its request reaches nothing.
    check_for_update: () => {
      const emitPhase = (payload: unknown) => state.bridge.emit("writ://update-status", { kind: "update:status", payload });
      emitPhase({ status: "checking" });
      setTimeout(() => emitPhase({ status: "failed", message: "error sending request for url (<redacted-url>)" }), 0);
      return null;
    },
    dismiss_update: () => null,

    // Folder
    get_workspace_root: () => state.config.workspace.root,
    clear_workspace_root: () => {
      state.config = { ...state.config, workspace: { ...state.config.workspace, root: null } };
      return null;
    },
    // The page has one folder, so the folder picker hands back that folder.
    pick_workspace_folder: () => {
      state.config = { ...state.config, workspace: { ...state.config.workspace, root: NOTES_ROOT } };
      return NOTES_ROOT;
    },
    get_notes_root: () => NOTES_ROOT,
    get_notes_folder: () => ({
      path: NOTES_ROOT,
      display_path: "~/Notes",
      fallback: null,
      sync_provider: null,
    }),
    list_workspace_dir: (args) => state.folder.list(String(args.dirPath)),
    workspace_index_status: (): IndexStatus => ({
      file_count: state.listFolderNotes().length,
      truncated: false,
      has_workspace: state.config.workspace.root !== null,
    }),
    get_inbox_path: () => state.config.inbox.path,
    // A page has no folder dialog; the picker closes without a pick.
    pick_inbox_folder: () => null,
    clear_inbox: () => {
      state.config = { ...state.config, inbox: { ...state.config.inbox, path: null } };
      return null;
    },
    list_inbox_files: () => [],

    list_transforms: () => [],
    preview_get_layout: () => null,
    preview_set_layout: () => null,
    preview_close: () => null,
    // Markdown renders in the editor. The HTML renderer needs the host's
    // writ-preview:// scheme, which a browser does not have.
    preview_list_renderers: (): Answer<"previewListRenderers"> => [
      {
        content_type: "markdown",
        capabilities: {
          supports_live_render: true,
          supports_print: true,
          max_safe_document_bytes: 50 * 1024 * 1024,
        },
      },
    ],
    check_spelling: () => [],
    list_default_app_types: () => [],
    classify_external_url: (args): Answer<"classifyExternalUrl"> => {
      const url = String(args.url);
      const isAllowed = /^https?:\/\//i.test(url);
      return { allowed: isAllowed, url: isAllowed ? url : null, reason: isAllowed ? null : "scheme", message: null };
    },
    open_external_url: (args) => {
      window.open(String(args.url), "_blank", "noopener");
      return null;
    },
  };
}

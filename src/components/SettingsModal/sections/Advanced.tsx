import { createSignal, onMount } from "solid-js";
import { configStore } from "../../../stores/global/config";
import { showToast } from "../../../stores/global/notifications";
import { copyStoragePath, fetchStorageInfo, revealStoragePath } from "../../../stores/global/storage";
import type { StorageInfo } from "../../../stores/global/storage";
import { FILE_MANAGER_NAME, SHOW_IN_FILE_MANAGER } from "../../../lib/platform";
import type { DefaultLayout } from "../../../types/config";
import Button from "../../Button/Button";
import { PathText, SectionLabel, clamp, parseFloatSafe, patchConfig } from "../controls";
import { SettingsRow } from "../SettingsRow";

/** What Writ keeps of a file's earlier versions. */
function VersionsRow() {
  return (
    <SettingsRow id="notes.versions">
      <span class="settings-notes-note" data-notes-retention>
        Writ keeps a version each time a file is saved, for up to 30 days or 200 versions.
      </span>
    </SettingsRow>
  );
}

/** The row naming the folder Writ keeps its own files in. */
function DataFolderRow() {
  const [info, setInfo] = createSignal<StorageInfo | null>(null);

  onMount(() => {
    void fetchStorageInfo()
      .then(setInfo)
      .catch(() => setInfo(null));
  });

  async function onShow() {
    try {
      await revealStoragePath();
    } catch {
      showToast(`Could not open ${FILE_MANAGER_NAME}`, "error");
    }
  }

  async function onCopy() {
    const path = info()?.dir;
    if (!path) return;
    try {
      await copyStoragePath(path);
      showToast("Path copied", "success");
    } catch {
      showToast("Could not copy the path", "error");
    }
  }

  return (
    <SettingsRow id="storage.location">
      <span class="settings-inbox-controls">
        <PathText
          class="settings-inbox-path"
          data-storage-path
          full={info()?.dir ?? ""}
          text={info()?.dir ?? "…"}
        />
        <Button data-action="storage-reveal" onClick={() => void onShow()}>
          {SHOW_IN_FILE_MANAGER}
        </Button>
        <Button data-action="storage-copy" onClick={() => void onCopy()}>
          Copy path
        </Button>
      </span>
    </SettingsRow>
  );
}

/**
 * How an HTML file opens, the two preview size limits, where Writ keeps its
 * own data and how long versions are kept. Running a file's own scripts is a
 * per-file choice ("Preview: Toggle scripts for this file"), not a switch.
 */
export default function AdvancedSection() {
  const preview = () => configStore.config().preview;

  function onHtmlLayoutChange(raw: string) {
    const layout = raw as DefaultLayout;
    void patchConfig((prev) => ({ ...prev, preview: { ...prev.preview, default_layout_html: layout } }));
  }

  // A live threshold above the refuse threshold describes a file Writ renders
  // live and refuses to render, so each value is held under the other.
  function onLiveLimitChange(raw: string) {
    const ceiling = Math.min(100, preview().render_refuse_threshold_mb);
    const value = clamp(parseFloatSafe(raw, preview().live_render_threshold_mb), 0.1, ceiling);
    void patchConfig((prev) => ({
      ...prev,
      preview: { ...prev.preview, live_render_threshold_mb: value },
    }));
  }

  function onHardLimitChange(raw: string) {
    const value = clamp(parseFloatSafe(raw, preview().render_refuse_threshold_mb), 1, 500);
    void patchConfig((prev) => ({
      ...prev,
      preview: {
        ...prev.preview,
        render_refuse_threshold_mb: value,
        live_render_threshold_mb: Math.min(prev.preview.live_render_threshold_mb, value),
      },
    }));
  }

  return (
    <div data-section="advanced">
      <SectionLabel section="advanced" />
      <SettingsRow id="preview.layout_html" labelFor="setting-layout-html">
        <select
          id="setting-layout-html"
          class="settings-select"
          data-setting="default_layout_html"
          value={preview().default_layout_html}
          onChange={(e) => onHtmlLayoutChange(e.currentTarget.value)}
        >
          <option value="source">The text</option>
          <option value="split">Text and preview</option>
          <option value="preview">Preview</option>
        </select>
      </SettingsRow>
      <SettingsRow
        id="preview.live_threshold"
        labelFor="setting-live-limit"
      >
        <span class="settings-inbox-controls">
          <input
            id="setting-live-limit"
            type="number"
            class="settings-input settings-input-number"
            data-setting="live_render_threshold_mb"
            value={preview().live_render_threshold_mb}
            min={0.1}
            max={100}
            step={0.5}
            onChange={(e) => onLiveLimitChange(e.currentTarget.value)}
          />
          <span class="settings-unit">MB</span>
        </span>
      </SettingsRow>
      <SettingsRow
        id="preview.refuse_threshold"
        labelFor="setting-hard-limit"
      >
        <span class="settings-inbox-controls">
          <input
            id="setting-hard-limit"
            type="number"
            class="settings-input settings-input-number"
            data-setting="render_refuse_threshold_mb"
            value={preview().render_refuse_threshold_mb}
            min={1}
            max={500}
            step={1}
            onChange={(e) => onHardLimitChange(e.currentTarget.value)}
          />
          <span class="settings-unit">MB</span>
        </span>
      </SettingsRow>
      <DataFolderRow />
      <VersionsRow />
    </div>
  );
}

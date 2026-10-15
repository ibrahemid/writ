import { For } from "solid-js";
import { configStore, EDITOR_FONT_MIN, EDITOR_FONT_MAX } from "../../../stores/global/config";
import type { MarkdownLayout, SidebarSectionId } from "../../../types/config";
import { SectionLabel, clamp, parseIntSafe, patchConfig } from "../controls";
import { ChordHints, FONT_SIZE_CHORDS, MARKDOWN_CHORDS, ProseSample } from "../previews";
import { SettingsRow, SwitchRow } from "../SettingsRow";

export interface SidebarRow {
  id: string;
  section: SidebarSectionId;
  setting: string;
}

const SIDEBAR_ROWS: readonly SidebarRow[] = [
  { id: "sidebar.folder", section: "folder", setting: "sidebar_folder" },
  { id: "sidebar.recent", section: "recent", setting: "sidebar_recent" },
];

export function SidebarSectionRow(props: { row: SidebarRow }) {
  return (
    <SwitchRow
      id={props.row.id}
      setting={props.row.setting}
      checked={!configStore.isSidebarSectionHidden(props.row.section)}
      onChange={() =>
        configStore.setSidebarSectionHidden(
          props.row.section,
          !configStore.isSidebarSectionHidden(props.row.section),
        )
      }
    />
  );
}

export default function EditorSection() {
  const cfg = () => configStore.config().editor;
  const spelling = () => configStore.config().spelling;

  function onFontSizeChange(raw: string) {
    const value = clamp(parseIntSafe(raw, cfg().font_size), EDITOR_FONT_MIN, EDITOR_FONT_MAX);
    void patchConfig((prev) => ({ ...prev, editor: { ...prev.editor, font_size: value } }));
  }

  function onTabSizeChange(raw: string) {
    const value = clamp(parseIntSafe(raw, cfg().tab_size), 1, 16);
    void patchConfig((prev) => ({ ...prev, editor: { ...prev.editor, tab_size: value } }));
  }

  function toggleEditor(
    key: "word_wrap" | "reading_width" | "markdown_editing" | "status_bar" | "status_bar_counts",
  ) {
    void patchConfig((prev) => ({ ...prev, editor: { ...prev.editor, [key]: !prev.editor[key] } }));
  }

  function onSpellingToggle() {
    void patchConfig((prev) => ({
      ...prev,
      spelling: { ...prev.spelling, enabled: !prev.spelling.enabled },
    }));
  }

  function onSpellingDialectChange(raw: string) {
    void patchConfig((prev) => ({ ...prev, spelling: { ...prev.spelling, dialect: raw } }));
  }

  function onMarkdownLayoutChange(raw: string) {
    const layout = raw as MarkdownLayout;
    void patchConfig((prev) => ({
      ...prev,
      preview: { ...prev.preview, default_layout_markdown: layout },
    }));
  }

  return (
    <div data-section="editor">
      <SectionLabel section="editor" />
      <SettingsRow
        id="editor.font_size"
        labelFor="setting-font-size"
        detail={
          <>
            <ChordHints hints={FONT_SIZE_CHORDS} />
            <ProseSample />
          </>
        }
      >
        <input
          id="setting-font-size"
          type="number"
          class="settings-input settings-input-number"
          data-setting="font_size"
          value={cfg().font_size}
          min={EDITOR_FONT_MIN}
          max={EDITOR_FONT_MAX}
          onChange={(e) => onFontSizeChange(e.currentTarget.value)}
        />
      </SettingsRow>
      <SettingsRow id="editor.tab_size" labelFor="setting-tab-size">
        <input
          id="setting-tab-size"
          type="number"
          class="settings-input settings-input-number"
          data-setting="tab_size"
          value={cfg().tab_size}
          min={1}
          max={16}
          onChange={(e) => onTabSizeChange(e.currentTarget.value)}
        />
      </SettingsRow>
      <SwitchRow
        id="editor.word_wrap"
        setting="word_wrap"
        checked={cfg().word_wrap}
        onChange={() => toggleEditor("word_wrap")}
      />
      <SwitchRow
        id="editor.reading_width"
        setting="reading_width"
        checked={cfg().reading_width}
        onChange={() => toggleEditor("reading_width")}
      />
      <SwitchRow
        id="editor.markdown_editing"
        setting="markdown_editing"
        checked={cfg().markdown_editing}
        detail={<ChordHints hints={MARKDOWN_CHORDS} />}
        onChange={() => toggleEditor("markdown_editing")}
      />
      <SettingsRow id="preview.layout_md" labelFor="setting-layout-md">
        <select
          id="setting-layout-md"
          class="settings-select"
          data-setting="default_layout_markdown"
          value={configStore.config().preview.default_layout_markdown}
          onChange={(e) => onMarkdownLayoutChange(e.currentTarget.value)}
        >
          <option value="inline">Inline</option>
          <option value="source">Source</option>
        </select>
      </SettingsRow>
      <SwitchRow
        id="editor.spelling"
        setting="spelling_enabled"
        checked={spelling().enabled}
        onChange={onSpellingToggle}
      />
      <SettingsRow
        id="editor.spelling_dialect"
        labelFor="setting-spelling-dialect"
        caution={spelling().enabled ? undefined : "Turn on Spell check to pick a dictionary."}
      >
        <select
          id="setting-spelling-dialect"
          class="settings-select"
          data-setting="spelling_dialect"
          disabled={!spelling().enabled}
          value={spelling().dialect}
          onChange={(e) => onSpellingDialectChange(e.currentTarget.value)}
        >
          <option value="american">English (US)</option>
          <option value="british">English (UK)</option>
          <option value="canadian">English (Canada)</option>
          <option value="australian">English (Australia)</option>
        </select>
      </SettingsRow>
      <SwitchRow
        id="editor.status_bar"
        setting="status_bar"
        checked={cfg().status_bar}
        onChange={() => toggleEditor("status_bar")}
      />
      <SwitchRow
        id="editor.status_bar_counts"
        setting="status_bar_counts"
        checked={cfg().status_bar_counts}
        onChange={() => toggleEditor("status_bar_counts")}
      />
      <For each={SIDEBAR_ROWS}>{(row) => <SidebarSectionRow row={row} />}</For>
    </div>
  );
}

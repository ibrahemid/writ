import { Show } from "solid-js";
import Button from "../../Button/Button";
import ShortcutEditor from "../../ShortcutEditor/ShortcutEditor";
import { SectionLabel } from "../controls";
import { isSearching, selectSection, setQuery } from "../state";
import { SettingsRow } from "../SettingsRow";

export default function ShortcutsSection() {
  function showShortcuts() {
    setQuery("");
    selectSection("shortcuts");
  }

  return (
    <div data-section="shortcuts">
      <SectionLabel section="shortcuts" />
      <Show
        when={!isSearching()}
        fallback={
          <SettingsRow id="shortcuts.edit">
            <Button data-action="show-shortcuts" onClick={showShortcuts}>
              Open
            </Button>
          </SettingsRow>
        }
      >
        <SettingsRow id="shortcuts.edit" layout="full">
          <ShortcutEditor />
        </SettingsRow>
      </Show>
    </div>
  );
}

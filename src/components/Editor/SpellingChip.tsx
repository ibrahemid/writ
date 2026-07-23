import { Show, createMemo } from "solid-js";
import { spellingStore } from "../../stores/global/spelling";
import { configStore } from "../../stores/global/config";
import { showAnchoredMenu } from "../ContextMenu/ContextMenu";
import { openSettings } from "../SettingsModal/SettingsModal";
import { openSpellingPreview } from "./SpellingPreview";

// Status-bar chip showing the live spell-check count. Hidden at zero and when
// the feature is off. Clicking opens a menu anchored to the chip.

export default function SpellingChip() {
  let ref: HTMLButtonElement | undefined;

  const visible = createMemo(
    () => configStore.config().spelling.enabled && spellingStore.count() > 0,
  );

  function openMenu() {
    if (!ref) return;
    const count = spellingStore.count();
    showAnchoredMenu(
      ref.getBoundingClientRect(),
      [
        { label: `Fix all (${count})`, action: () => spellingStore.fixAll() },
        { label: "Preview…", action: () => openSpellingPreview() },
        { label: "Spelling settings", action: () => openSettings("editor", "editor.spelling"), separator: true },
      ],
      ref,
    );
  }

  return (
    <Show when={visible()}>
      <button
        ref={ref}
        type="button"
        class="statusbar-chip spelling-chip"
        onClick={openMenu}
        title="Spelling"
      >
        {spellingStore.count()} spelling
      </button>
    </Show>
  );
}

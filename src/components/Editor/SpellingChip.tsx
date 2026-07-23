import { Show, createMemo } from "solid-js";
import { spellingStore } from "../../stores/global/spelling";
import { configStore } from "../../stores/global/config";
import { showAnchoredMenu } from "../ContextMenu/ContextMenu";
import { openSettings } from "../SettingsModal/SettingsModal";
import { openSpellingPreview } from "./SpellingPreview";

// Status-bar spell-check item and switch. Shown for any eligible buffer
// (Normal mode, under the size cap), hidden otherwise. Three states: off
// (muted), on with no issues (plain label), on with issues (the count).
// Clicking always opens a menu that carries the on/off switch.

interface MenuEntry {
  label: string;
  action: () => void;
  separator?: boolean;
}

export default function SpellingChip() {
  let ref: HTMLButtonElement | undefined;

  const eligible = () => spellingStore.eligible();
  const enabled = () => configStore.config().spelling.enabled;
  const count = () => spellingStore.count();

  const label = createMemo(() => {
    if (!enabled()) return "Spelling off";
    const n = count();
    return n > 0 ? `${n} spelling` : "Spelling";
  });

  function setEnabled(next: boolean) {
    const current = configStore.config();
    void configStore.save({
      ...current,
      spelling: { ...current.spelling, enabled: next },
    });
  }

  function menuItems(): MenuEntry[] {
    const settings: MenuEntry = {
      label: "Spelling settings",
      action: () => openSettings("editor", "editor.spelling"),
      separator: true,
    };
    if (!enabled()) {
      return [{ label: "Turn on spelling", action: () => setEnabled(true) }, settings];
    }
    const items: MenuEntry[] = [{ label: "Turn off spelling", action: () => setEnabled(false) }];
    if (count() > 0) {
      items.push({ label: `Fix all (${count()})`, action: () => spellingStore.fixAll() });
      items.push({ label: "Preview…", action: () => openSpellingPreview() });
    }
    items.push(settings);
    return items;
  }

  function openMenu() {
    if (!ref) return;
    showAnchoredMenu(ref.getBoundingClientRect(), menuItems(), ref);
  }

  return (
    <Show when={eligible()}>
      <button
        ref={ref}
        type="button"
        class="statusbar-chip spelling-chip"
        classList={{ "spelling-chip--off": !enabled() }}
        onClick={openMenu}
        title="Spelling"
        aria-label={label()}
      >
        {label()}
      </button>
    </Show>
  );
}

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

function partToGlyph(part: string, mac: boolean): string {
  if (part === "CmdOrCtrl") return mac ? "⌘" : "Ctrl";
  if (part === "Shift") return mac ? "⇧" : "Shift";
  if (part === "Alt") return mac ? "⌥" : "Alt";
  if (part === "Ctrl") return mac ? "⌃" : "Ctrl";
  return part;
}

export function formatKeybinding(binding: string | undefined, opts?: { isMac?: boolean }): string {
  if (!binding) return "";
  const mac = opts?.isMac ?? IS_MAC;

  if (binding === "Shift+Shift") return mac ? "⇧ ⇧" : "Shift Shift";

  return binding
    .split("+")
    .map((part) => partToGlyph(part, mac))
    .join(mac ? "" : "+");
}

export function keybindingSegments(binding: string | undefined, opts?: { isMac?: boolean }): string[] {
  if (!binding) return [];
  const mac = opts?.isMac ?? IS_MAC;
  if (binding === "Shift+Shift") {
    return mac ? ["⇧", "⇧"] : ["Shift", "Shift"];
  }
  return binding.split("+").map((part) => partToGlyph(part, mac));
}

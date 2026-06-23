const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

// Formats a byte count for compact display (e.g. "0 B", "512 B", "1.2 KB",
// "3 MB"). Uses 1024-based units, one decimal place below 10 of a unit and
// none at or above it, so sizes stay short and stable in the sidebar.
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) return `${Math.round(value)} B`;
  const rounded = value < 10 ? Math.round(value * 10) / 10 : Math.round(value);
  return `${rounded} ${UNITS[unit]}`;
}

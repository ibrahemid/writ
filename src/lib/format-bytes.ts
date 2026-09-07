const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

// Formats a byte count for display (e.g. "0 B", "512 B", "1.2 KB", "3 MB").
// Decimal units, matching what the file manager reports for the same file, with
// one decimal place below 10 of a unit and none at or above it.
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  if (unit === 0) return `${Math.round(value)} B`;
  const rounded = value < 10 ? Math.round(value * 10) / 10 : Math.round(value);
  return `${rounded} ${UNITS[unit]}`;
}

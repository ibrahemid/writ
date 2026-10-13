import type { FileOpenMode } from "../../src/types/buffer";

// writ_core::file_ops, which decides how an open answers: the size tiers, the
// binary sniff and the hex dump a binary file is shown as. The page answers
// from these copies; demo/__tests__/rust-tables.test.ts holds them to the
// Rust source.

const MiB = 1024 * 1024;

export const THRESHOLD_NORMAL_BYTES = 5 * MiB;
export const THRESHOLD_LARGE_BYTES = 50 * MiB;
export const THRESHOLD_MAX_BYTES = 500 * MiB;
export const BINARY_CHECK_BYTES = 8192;
export const HEX_DUMP_MAX_BYTES = 10 * MiB;
export const ERR_OPEN_TOO_LARGE = "ERR_OPEN_TOO_LARGE";

/** file_ops::classify_file: the tier for a size and the binary sniff. */
export function classifyFile(sizeBytes: number, isBinary: boolean): FileOpenMode {
  if (isBinary) return { kind: "Binary" };
  if (sizeBytes <= THRESHOLD_NORMAL_BYTES) return { kind: "Normal" };
  if (sizeBytes <= THRESHOLD_LARGE_BYTES) return { kind: "LargeFile" };
  return { kind: "LargeFileConfirm" };
}

/** file_ops::format_bytes. */
function formatBinaryBytes(bytes: number): string {
  const GiB = 1024 * MiB;
  if (bytes >= GiB) return `${(bytes / GiB).toFixed(1)} GiB`;
  if (bytes >= MiB) return `${(bytes / MiB).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KiB`;
  return `${bytes} B`;
}

/** file_ops::classify_size: classify_file with the refusal past the size Writ opens. */
export function classifySize(sizeBytes: number, isBinary: boolean): FileOpenMode {
  if (sizeBytes > THRESHOLD_MAX_BYTES) {
    return {
      kind: "Refused",
      reason: `${ERR_OPEN_TOO_LARGE}: ${formatBinaryBytes(sizeBytes)} is over the ${formatBinaryBytes(THRESHOLD_MAX_BYTES)} limit`,
    };
  }
  return classifyFile(sizeBytes, isBinary);
}

/** FileOpenMode::once_open: a file that was asked about is a large file once open. */
export function onceOpen(mode: FileOpenMode): FileOpenMode {
  return mode.kind === "LargeFileConfirm" ? { kind: "LargeFile" } : mode;
}

/** file_ops::sniff_binary: a NUL byte in the first BINARY_CHECK_BYTES. */
export function sniffsBinary(bytes: Uint8Array): boolean {
  return bytes.subarray(0, BINARY_CHECK_BYTES).includes(0);
}

const hexByte = (byte: number) => byte.toString(16).padStart(2, "0");

/** file_ops::generate_hex_dump. */
export function formatHexDump(data: Uint8Array, totalBytes: number): string {
  const capped = data.subarray(0, Math.min(data.length, HEX_DUMP_MAX_BYTES));
  const rows: string[] = [];
  for (let offset = 0; offset < capped.length; offset += 16) {
    const chunk = capped.subarray(offset, offset + 16);
    let row = `${offset.toString(16).padStart(8, "0")}  `;
    chunk.forEach((byte, i) => {
      if (i === 8) row += " ";
      row += `${hexByte(byte)} `;
    });
    for (let i = 0; i < 16 - chunk.length; i++) {
      if (chunk.length + i === 8) row += " ";
      row += "   ";
    }
    let gutter = "";
    for (const byte of chunk) gutter += byte >= 0x21 && byte <= 0x7e || byte === 0x20 ? String.fromCharCode(byte) : ".";
    rows.push(`${row} |${gutter}|\n`);
  }
  let out = rows.join("");
  if (totalBytes > HEX_DUMP_MAX_BYTES) {
    out += `\n[ truncated — showing first ${formatBinaryBytes(HEX_DUMP_MAX_BYTES)} of ${formatBinaryBytes(totalBytes)} ]\n`;
  }
  return out;
}

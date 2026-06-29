export interface HexRow {
  off: string;
  left: string;
  right: string;
  ascii: string;
}

export function genHex(): HexRow[] {
  const signature = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
  ];
  const bytes = signature.slice();
  let seed = 0x4a3f;
  for (let i = 16; i < 16 * 28; i++) {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    bytes.push((seed >> 13) & 0xff);
  }
  const rows: HexRow[] = [];
  for (let off = 0; off < bytes.length; off += 16) {
    const row = bytes.slice(off, off + 16);
    const hex = row.map((b) => b.toString(16).padStart(2, '0'));
    const left = hex.slice(0, 8).join(' ');
    const right = hex.slice(8).join(' ');
    const ascii = row.map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.')).join('');
    rows.push({ off: off.toString(16).padStart(8, '0'), left, right, ascii });
  }
  return rows;
}

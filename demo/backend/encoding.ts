import type { LineEnding } from "../../src/types/buffer";
import { ENCODING_CHOICES, encodingName } from "../../src/lib/text-encoding";

const BOM_SUFFIX = " BOM";
export const UTF_8 = "UTF-8";

/** writ_core::text_encoding: the byte order marks an encoding can start with. */
const MARKS: Record<string, readonly number[]> = {
  "UTF-8": [0xef, 0xbb, 0xbf],
  "UTF-16LE": [0xff, 0xfe],
  "UTF-16BE": [0xfe, 0xff],
};

// A page decodes every WHATWG encoding but encodes only UTF-8. UTF-16 and the
// single-byte encodings are encoded here; these get ASCII text only.
const MULTIBYTE = new Set(["shift_jis", "euc-jp", "iso-2022-jp", "gbk", "gb18030", "big5", "euc-kr"]);

/** writ_core::text_encoding::MAX_NAMED_CHARACTERS. */
const MAX_NAMED_CHARACTERS = 8;

/** A label `TextEncoding::from_label` reads as no encoding. */
export class UnknownEncodingError extends Error {
  constructor(readonly label: string) {
    super(`${JSON.stringify(label)} is not an encoding`);
    this.name = "UnknownEncodingError";
  }
}

/** StorageError::NotInEncoding: bytes that are not text in the encoding they are read in. */
export class NotInEncodingError extends Error {
  constructor(readonly encoding: string) {
    super(`the file is not ${encoding} text`);
    this.name = "NotInEncodingError";
  }
}

/** writ_core::text_encoding::EncodingLossy: characters the encoding has no bytes for. */
export class EncodingLossyError extends Error {
  constructor(
    readonly encoding: string,
    readonly characters: readonly string[],
  ) {
    super(`${encoding} has no bytes for ${characters.map(formatCodePoint).join(" ")}`);
    this.name = "EncodingLossyError";
  }
}

/** Text past ASCII in a multibyte encoding, which the page has no encoder for. */
export class EncodingUnwritableError extends Error {
  constructor(readonly encoding: string) {
    super(`the page has no ${encoding} encoder for text past ASCII`);
    this.name = "EncodingUnwritableError";
  }
}

function formatCodePoint(character: string): string {
  return `U+${(character.codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, "0")}`;
}

/** TextEncoding::from_label: the encoding's name, with " BOM" kept where the encoding has a mark. */
export function resolveEncoding(label: string): string | null {
  let canonical: string;
  try {
    canonical = new TextDecoder(encodingName(label).trim()).encoding;
  } catch {
    return null;
  }
  const name = ENCODING_CHOICES.find((choice) => choice.name.toLowerCase() === canonical)?.name ?? canonical;
  if (!label.endsWith(BOM_SUFFIX)) return name;
  return name in MARKS ? `${name}${BOM_SUFFIX}` : null;
}

/** TextEncoding::with_bom_of: `encoding`, marked when `bytes` start with its byte order mark. */
export function withBomOf(encoding: string, bytes: Uint8Array): string {
  const name = encodingName(encoding);
  const mark = MARKS[name];
  const starts = mark !== undefined && bytes.length >= mark.length && mark.every((byte, i) => bytes[i] === byte);
  return starts ? `${name}${BOM_SUFFIX}` : name;
}

/** TextEncoding::decode: the text `bytes` hold in `encoding`. */
export function decodeIn(bytes: Uint8Array, encoding: string): string {
  try {
    return new TextDecoder(encodingName(encoding), { fatal: true }).decode(bytes);
  } catch (error) {
    if (error instanceof TypeError) throw new NotInEncodingError(encoding);
    throw error;
  }
}

const singleByteTables = new Map<string, Map<string, number>>();

/** Each character a single-byte encoding holds, to the byte it holds it as. */
function getSingleByteTable(name: string): Map<string, number> {
  let table = singleByteTables.get(name);
  if (!table) {
    const decoder = new TextDecoder(name);
    table = new Map();
    for (let byte = 0xff; byte >= 0; byte--) {
      const character = decoder.decode(Uint8Array.of(byte));
      if (character !== "�") table.set(character, byte);
    }
    singleByteTables.set(name, table);
  }
  return table;
}

function encodeSingleByte(text: string, encoding: string): Uint8Array {
  const table = getSingleByteTable(encodingName(encoding));
  const out: number[] = [];
  const unmappable: string[] = [];
  for (const character of text) {
    const byte = table.get(character);
    if (byte !== undefined) out.push(byte);
    else if (unmappable.length < MAX_NAMED_CHARACTERS && !unmappable.includes(character)) unmappable.push(character);
  }
  if (unmappable.length > 0) throw new EncodingLossyError(encoding, unmappable);
  return Uint8Array.from(out);
}

function encodeUtf16(text: string, littleEndian: boolean): Uint8Array {
  const out = new Uint8Array(text.length * 2);
  const view = new DataView(out.buffer);
  for (let i = 0; i < text.length; i++) view.setUint16(i * 2, text.charCodeAt(i), littleEndian);
  return out;
}

/** TextEncoding::encode: `text` as the bytes a file in `encoding` holds, its byte order mark first. */
export function encodeIn(text: string, encoding: string): Uint8Array {
  const name = encodingName(encoding);
  let body: Uint8Array;
  if (name === "UTF-16LE" || name === "UTF-16BE") {
    body = encodeUtf16(text, name === "UTF-16LE");
  } else if (name === UTF_8 || /^[\x00-\x7f]*$/.test(text)) {
    body = new TextEncoder().encode(text);
  } else if (MULTIBYTE.has(name.toLowerCase())) {
    throw new EncodingUnwritableError(encoding);
  } else {
    body = encodeSingleByte(text, encoding);
  }
  const mark = encoding.endsWith(BOM_SUFFIX) ? (MARKS[name] ?? []) : [];
  if (mark.length === 0) return body;
  const out = new Uint8Array(mark.length + body.length);
  out.set(mark);
  out.set(body, mark.length);
  return out;
}

/** LineEnding::detect: CRLF when it ends more lines than LF alone does. */
export function detectLineEnding(text: string): LineEnding {
  const crlf = text.match(/\r\n/g)?.length ?? 0;
  const lf = (text.match(/\n/g)?.length ?? 0) - crlf;
  return crlf > lf ? "crlf" : "lf";
}

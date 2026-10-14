// Text encodings as the editor names them (ADR-049).
//
// Rust carries an encoding as its WHATWG name, followed by " BOM" when the file
// starts with a byte order mark ("UTF-8", "UTF-16LE BOM", "windows-1252"). The
// names below are the ones Reopen with Encoding sends back; Rust reads each
// with `TextEncoding::from_label`.

/** One encoding Reopen with Encoding offers. */
export interface EncodingChoice {
  /** The WHATWG name Rust reads. */
  name: string;
  /** What the status bar shows. */
  short: string;
  /** The row in the list. */
  label: string;
  /** Other words the row is found by. */
  terms: readonly string[];
}

function choice(name: string, short: string, region: string | null, terms: string[] = []) {
  return {
    name,
    short,
    label: region === null ? short : `${region} (${short})`,
    terms,
  };
}

export const ENCODING_CHOICES: readonly EncodingChoice[] = [
  choice("UTF-8", "UTF-8", null, ["unicode", "utf8"]),
  choice("UTF-16LE", "UTF-16 LE", null, ["unicode", "utf16", "little endian"]),
  choice("UTF-16BE", "UTF-16 BE", null, ["unicode", "utf16", "big endian"]),
  choice("windows-1252", "Windows-1252", "Western", ["latin-1", "latin1", "iso-8859-1", "cp1252"]),
  choice("ISO-8859-15", "ISO-8859-15", "Western", ["latin-9", "latin9"]),
  choice("macintosh", "Mac OS Roman", "Western", ["mac", "roman"]),
  choice("windows-1250", "Windows-1250", "Central European", ["cp1250"]),
  choice("ISO-8859-2", "ISO-8859-2", "Central European", ["latin-2", "latin2"]),
  choice("windows-1251", "Windows-1251", "Cyrillic", ["cp1251", "russian"]),
  choice("KOI8-R", "KOI8-R", "Cyrillic", ["russian"]),
  choice("KOI8-U", "KOI8-U", "Cyrillic", ["ukrainian"]),
  choice("IBM866", "IBM 866", "Cyrillic", ["cp866", "dos"]),
  choice("ISO-8859-5", "ISO-8859-5", "Cyrillic"),
  choice("windows-1253", "Windows-1253", "Greek", ["cp1253"]),
  choice("ISO-8859-7", "ISO-8859-7", "Greek"),
  choice("windows-1254", "Windows-1254", "Turkish", ["cp1254", "latin-5"]),
  choice("windows-1255", "Windows-1255", "Hebrew", ["cp1255"]),
  choice("ISO-8859-8", "ISO-8859-8", "Hebrew"),
  choice("windows-1256", "Windows-1256", "Arabic", ["cp1256"]),
  choice("ISO-8859-6", "ISO-8859-6", "Arabic"),
  choice("windows-1257", "Windows-1257", "Baltic", ["cp1257"]),
  choice("ISO-8859-4", "ISO-8859-4", "Baltic"),
  choice("ISO-8859-13", "ISO-8859-13", "Baltic"),
  choice("windows-1258", "Windows-1258", "Vietnamese", ["cp1258"]),
  choice("windows-874", "Windows-874", "Thai", ["tis-620"]),
  choice("Shift_JIS", "Shift JIS", "Japanese", ["sjis", "cp932"]),
  choice("EUC-JP", "EUC-JP", "Japanese"),
  choice("ISO-2022-JP", "ISO-2022-JP", "Japanese", ["jis"]),
  choice("GBK", "GBK", "Chinese Simplified", ["gb2312", "cp936"]),
  choice("gb18030", "GB 18030", "Chinese Simplified"),
  choice("Big5", "Big5", "Chinese Traditional", ["cp950"]),
  choice("EUC-KR", "EUC-KR", "Korean", ["cp949"]),
];

const BOM_SUFFIX = " BOM";

/** The encoding's name without the byte order mark Rust appends. */
export function encodingName(encoding: string): string {
  return encoding.endsWith(BOM_SUFFIX) ? encoding.slice(0, -BOM_SUFFIX.length) : encoding;
}

/** The choice that stands for `encoding`, when the list has one. */
export function encodingChoiceOf(encoding: string): EncodingChoice | undefined {
  const name = encodingName(encoding).toLowerCase();
  return ENCODING_CHOICES.find((entry) => entry.name.toLowerCase() === name);
}

/**
 * What the status bar shows for `encoding`: the short name, with the mark for
 * UTF-8, where a byte order mark is the exception. An encoding outside the
 * list shows under Rust's own name.
 */
export function encodingLabel(encoding: string): string {
  const entry = encodingChoiceOf(encoding);
  const short = entry?.short ?? encodingName(encoding);
  if (encoding.endsWith(BOM_SUFFIX) && encodingName(encoding) === "UTF-8") {
    return `${short} with BOM`;
  }
  return short;
}

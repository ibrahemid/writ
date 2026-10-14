use std::io::Write;

use writ_core::file_ops::{classify_path, is_binary_bytes, FileOpenMode};
use writ_core::hash::comparison_digest_hex;
use writ_core::notes::line_ending::LineEnding;
use writ_core::text_encoding::{
    code_points, declared, decode_detected, decode_detected_owned, decode_file, detect,
    EncodingLossy, TextEncoding,
};

// A CSV as Excel on Windows writes it: Windows-1252, CRLF.
const WINDOWS_1252_CSV: &[u8] = b"name,city,note\r\nCaf\xe9 Nord,M\xfcnchen,cr\xe8me br\xfbl\xe9e\r\nNa\xefve,Z\xfcrich,\xa9 2024\r\n";

// "Grüße aus Köln, schöne Straße." in ISO-8859-1.
const LATIN_1: &[u8] =
    b"Gr\xfc\xdfe aus K\xf6ln, sch\xf6ne Stra\xdfe.\n\xc4rger \xfcber \xd6l und \xc9t\xe9.\n";

// "日本語のテキストです。これはテストです。\n" in Shift_JIS.
const SHIFT_JIS: &[u8] = b"\x93\xfa\x96\x7b\x8c\xea\x82\xcc\x83\x65\x83\x4c\x83\x58\x83\x67\x82\xc5\x82\xb7\x81\x42\x82\xb1\x82\xea\x82\xcd\x83\x65\x83\x58\x83\x67\x82\xc5\x82\xb7\x81\x42\n";
const SHIFT_JIS_TEXT: &str = "日本語のテキストです。これはテストです。\n";

// "中文文本，这是一个测试。我们的文件。\n" in GBK.
const GBK: &[u8] = b"\xd6\xd0\xce\xc4\xce\xc4\xb1\xbe\xa3\xac\xd5\xe2\xca\xc7\xd2\xbb\xb8\xf6\xb2\xe2\xca\xd4\xa1\xa3\xce\xd2\xc3\xc7\xb5\xc4\xce\xc4\xbc\xfe\xa1\xa3\n";
const GBK_TEXT: &str = "中文文本，这是一个测试。我们的文件。\n";

// What PowerShell 5.1's `dir > out.txt` writes: UTF-16LE with a byte order
// mark, CRLF.
const POWERSHELL_TEXT: &str =
    "\r\n    Directory: C:\\Users\\ibra\r\n\r\nMode   Name\r\n----   ----\r\nd----  Documents\r\n-a---  café.txt\r\n";

fn utf16(text: &str, little: bool, bom: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if bom {
        out.extend_from_slice(if little { b"\xff\xfe" } else { b"\xfe\xff" });
    }
    for unit in text.encode_utf16() {
        out.extend_from_slice(&if little {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        });
    }
    out
}

// What a save does to text a file was read as: the editor's LF text gets the
// file's ending back and is encoded in the file's encoding.
fn round_trip(bytes: &[u8]) -> (TextEncoding, String, Vec<u8>) {
    let decoded = decode_detected(bytes);
    let ending = LineEnding::detect(&decoded.text);
    let editor_text = LineEnding::normalise(&decoded.text).into_owned();
    let saved = decoded
        .encoding
        .encode(&ending.apply(&editor_text))
        .expect("the text the file held encodes back")
        .into_owned();
    (decoded.encoding, editor_text, saved)
}

#[test]
fn a_windows_1252_csv_reads_as_windows_1252_and_saves_byte_for_byte() {
    let (encoding, text, saved) = round_trip(WINDOWS_1252_CSV);
    assert_eq!(encoding.name(), "windows-1252");
    assert!(text.contains("Café Nord,München,crème brûlée"));
    assert_eq!(saved, WINDOWS_1252_CSV);
}

#[test]
fn a_latin_1_file_reads_and_saves_byte_for_byte() {
    let (encoding, text, saved) = round_trip(LATIN_1);
    assert_eq!(encoding.name(), "windows-1252");
    assert!(text.starts_with("Grüße aus Köln"));
    assert_eq!(saved, LATIN_1);
}

#[test]
fn every_byte_windows_1252_reads_writes_back_as_itself() {
    // ISO-8859-1 is read as Windows-1252 (the WHATWG table), which decodes
    // all 256 bytes, so no Latin-1 file loses a byte to the substitution.
    let bytes: Vec<u8> = (0u8..=255).collect();
    let encoding = TextEncoding::from_label("iso-8859-1").expect("a known label");
    assert_eq!(encoding, TextEncoding::windows_1252());
    let text = encoding.decode(&bytes).expect("every byte has a character");
    assert_eq!(encoding.encode(&text).expect("and back"), bytes.as_slice());
}

#[test]
fn a_shift_jis_file_reads_and_saves_byte_for_byte() {
    let (encoding, text, saved) = round_trip(SHIFT_JIS);
    assert_eq!(encoding.name(), "Shift_JIS");
    assert_eq!(text, SHIFT_JIS_TEXT);
    assert_eq!(saved, SHIFT_JIS);
}

#[test]
fn a_gbk_file_reads_and_saves_byte_for_byte() {
    let (encoding, text, saved) = round_trip(GBK);
    assert_eq!(encoding.name(), "GBK");
    assert_eq!(text, GBK_TEXT);
    assert_eq!(saved, GBK);
}

#[test]
fn utf16_with_a_byte_order_mark_reads_and_saves_byte_for_byte() {
    for little in [true, false] {
        let bytes = utf16(POWERSHELL_TEXT, little, true);
        let (encoding, text, saved) = round_trip(&bytes);
        assert_eq!(
            encoding.name(),
            if little { "UTF-16LE" } else { "UTF-16BE" }
        );
        assert!(encoding.has_bom());
        assert_eq!(text, LineEnding::normalise(POWERSHELL_TEXT));
        assert_eq!(saved, bytes, "little endian: {little}");
    }
}

#[test]
fn utf16_without_a_byte_order_mark_reads_and_saves_byte_for_byte() {
    for little in [true, false] {
        let bytes = utf16(POWERSHELL_TEXT, little, false);
        let (encoding, text, saved) = round_trip(&bytes);
        assert_eq!(
            encoding.name(),
            if little { "UTF-16LE" } else { "UTF-16BE" }
        );
        assert!(!encoding.has_bom());
        assert_eq!(text, LineEnding::normalise(POWERSHELL_TEXT));
        assert_eq!(saved, bytes, "little endian: {little}");
    }
}

#[test]
fn utf16_keeps_its_crlf_endings() {
    let bytes = utf16("one\r\ntwo\r\nthree\r\n", true, true);
    let decoded = decode_detected(&bytes);
    assert_eq!(LineEnding::detect(&decoded.text), LineEnding::CrLf);
}

#[test]
fn utf8_with_a_byte_order_mark_keeps_it() {
    let bytes = b"\xef\xbb\xbfcaf\xc3\xa9\n";
    let (encoding, text, saved) = round_trip(bytes);
    assert_eq!(encoding.label(), "UTF-8 BOM");
    assert_eq!(text, "café\n");
    assert_eq!(saved, bytes);
}

#[test]
fn utf8_without_a_mark_is_utf8_and_borrowed() {
    let bytes = "naïve résumé\n".as_bytes();
    assert_eq!(detect(bytes), TextEncoding::utf8());
    let text = TextEncoding::utf8().decode(bytes).expect("utf-8");
    assert!(matches!(text, std::borrow::Cow::Borrowed(_)));
}

#[test]
fn a_character_the_encoding_cannot_hold_refuses_the_encode_and_names_it() {
    let error = TextEncoding::windows_1252()
        .encode("café 😀, 中 and 😀 again, € is fine")
        .expect_err("emoji and CJK have no Windows-1252 bytes");
    assert_eq!(
        error,
        EncodingLossy {
            encoding: TextEncoding::windows_1252(),
            characters: vec!['😀', '中'],
        }
    );
    assert_eq!(code_points(&error.characters), "U+1F600 U+4E2D");
    assert_eq!(
        error.to_string(),
        "windows-1252 has no bytes for U+1F600 U+4E2D"
    );
}

#[test]
fn utf8_and_utf16_hold_every_character() {
    let text = "😀 中 é";
    for label in ["UTF-8", "UTF-8 BOM", "UTF-16LE BOM", "UTF-16BE"] {
        let encoding = TextEncoding::from_label(label).expect("a known label");
        let bytes = encoding.encode(text).expect("unicode holds everything");
        assert_eq!(encoding.decode(&bytes).expect("and reads back"), text);
    }
}

#[test]
fn a_decode_that_would_replace_bytes_is_refused() {
    let error = TextEncoding::utf8()
        .decode(b"caf\xe9")
        .expect_err("0xE9 alone is not UTF-8");
    assert_eq!(error.encoding, TextEncoding::utf8());
    let shift_jis = TextEncoding::from_label("Shift_JIS").expect("known");
    assert!(
        shift_jis.decode(b"\x81").is_err(),
        "a lead byte with no trail"
    );
}

#[test]
fn the_same_bytes_reopened_in_another_encoding_read_differently() {
    let as_guessed = decode_detected(SHIFT_JIS);
    let as_1252 = TextEncoding::windows_1252()
        .decode(SHIFT_JIS)
        .expect("windows-1252 reads every byte");
    assert_ne!(as_guessed.text, as_1252);
    assert_eq!(
        TextEncoding::windows_1252()
            .encode(&as_1252)
            .expect("and back"),
        SHIFT_JIS
    );
}

#[test]
fn labels_read_back_and_unknown_ones_do_not() {
    for label in [
        "UTF-8",
        "UTF-8 BOM",
        "UTF-16LE",
        "UTF-16LE BOM",
        "UTF-16BE BOM",
        "windows-1252",
        "Shift_JIS",
        "GBK",
        "EUC-KR",
    ] {
        let encoding = TextEncoding::from_label(label).expect(label);
        assert_eq!(encoding.label(), label);
        let json = serde_json::to_string(&encoding).expect("serialises");
        assert_eq!(json, format!("\"{label}\""));
        let back: TextEncoding = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back, encoding);
    }
    assert_eq!(
        TextEncoding::from_label("latin1"),
        Some(TextEncoding::windows_1252())
    );
    assert_eq!(TextEncoding::from_label("windows-1252 BOM"), None);
    assert_eq!(TextEncoding::from_label("replacement"), None);
    assert_eq!(TextEncoding::from_label("klingon"), None);
    assert!(serde_json::from_str::<TextEncoding>("\"klingon\"").is_err());
}

#[test]
fn the_mark_follows_the_bytes() {
    let utf8 = TextEncoding::utf8();
    assert!(utf8.with_bom_of(b"\xef\xbb\xbfx").has_bom());
    assert!(!utf8.with_bom_of(b"x").has_bom());
    assert!(!TextEncoding::windows_1252()
        .with_bom_of(b"\xef\xbb\xbfx")
        .has_bom());
}

#[test]
fn the_comparison_digest_is_over_the_text_the_editor_holds() {
    let bytes = b"caf\xe9\r\nna\xefve\r\n";
    assert_eq!(
        TextEncoding::windows_1252().comparison_digest_hex(bytes),
        comparison_digest_hex("café\nnaïve\n".as_bytes())
    );
    let utf16 = utf16("café\r\n", true, true);
    let encoding = TextEncoding::from_label("UTF-16LE BOM").expect("known");
    assert_eq!(
        encoding.comparison_digest_hex(&utf16),
        comparison_digest_hex("café\n".as_bytes())
    );
    // A UTF-8 file is hashed as it always was.
    assert_eq!(
        TextEncoding::utf8().comparison_digest_hex(b"x\r\n"),
        comparison_digest_hex(b"x\r\n")
    );
}

#[test]
fn utf16_is_text_to_the_binary_sniff_and_binary_data_is_not() {
    assert!(!is_binary_bytes(&utf16(POWERSHELL_TEXT, true, true)));
    assert!(!is_binary_bytes(&utf16(POWERSHELL_TEXT, true, false)));
    assert!(!is_binary_bytes(&utf16(POWERSHELL_TEXT, false, false)));

    let png: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x01\x00\x00\x00\x01\x00\x08\x06\x00\x00\x00\x5c\x72\xa8\x66\x00\x00\x00\x04sBIT\x08\x08\x08\x08";
    assert!(is_binary_bytes(png));
    assert_eq!(declared(png), None);
    // Little-endian integers: zeros on the odd side, but control characters
    // once read as UTF-16.
    let integers: Vec<u8> = (1u16..200).flat_map(u16::to_le_bytes).collect();
    assert!(is_binary_bytes(&integers));
    assert!(is_binary_bytes(&[0u8; 64]));
}

#[test]
fn a_utf16_file_classifies_as_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    let mut file = std::fs::File::create(&path).expect("create");
    file.write_all(&utf16(POWERSHELL_TEXT, true, true))
        .expect("write");
    drop(file);
    assert_eq!(
        classify_path(&path).expect("classifies").mode,
        FileOpenMode::Normal
    );
}

#[test]
fn a_detected_file_is_never_read_with_a_replacement_character() {
    // Bytes with no NUL that are not UTF-8: whatever the guess, the text
    // read decodes back without loss, since a guess that cannot decode every
    // byte gives way to Windows-1252.
    let mut state = 0x2545_f491_u32;
    for _ in 0..200 {
        let bytes: Vec<u8> = (0..300)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state % 255) as u8 + 1
            })
            .collect();
        let decoded = decode_detected(&bytes);
        let strict = decoded
            .encoding
            .decode(&bytes)
            .expect("the encoding read decodes every byte");
        assert_eq!(strict, decoded.text);
    }
}

#[test]
fn the_owned_read_gives_the_same_text_as_the_borrowed_one() {
    let samples: Vec<Vec<u8>> = vec![
        b"plain ascii\n".to_vec(),
        "caf\u{e9} in utf-8\n".as_bytes().to_vec(),
        b"\xef\xbb\xbfwith a mark\n".to_vec(),
        WINDOWS_1252_CSV.to_vec(),
        SHIFT_JIS.to_vec(),
        utf16(POWERSHELL_TEXT, true, true),
    ];
    for bytes in samples {
        let borrowed = decode_detected(&bytes);
        let (encoding, text) = decode_detected_owned(bytes.clone());
        assert_eq!(encoding, borrowed.encoding);
        assert_eq!(text, borrowed.text);
    }
}

#[test]
fn a_file_read_without_a_row_is_text_or_binary() {
    let decoded = decode_file(WINDOWS_1252_CSV).expect("text");
    assert_eq!(decoded.encoding, TextEncoding::windows_1252());
    assert!(decode_file(&utf16(POWERSHELL_TEXT, false, true)).is_some());
    assert!(decode_file(b"\x00\x01\x02\x03binary").is_none());
}

#[test]
fn every_encoding_the_editor_offers_is_one_rust_reads_under_the_same_name() {
    // The list behind Reopen with Encoding (`src/lib/text-encoding.ts`) sends
    // these names back to `reopen_with_encoding`, and marks the row whose name
    // is the row's encoding.
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/lib/text-encoding.ts"
    ))
    .expect("the editor's list");
    let names: Vec<&str> = source
        .match_indices("choice(\"")
        .map(|(at, opening)| {
            let rest = &source[at + opening.len()..];
            &rest[..rest.find('"').expect("a closing quote")]
        })
        .collect();
    assert!(names.len() >= 30, "the list was not found: {names:?}");
    for name in names {
        let encoding = TextEncoding::from_label(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(encoding.name(), name);
    }
}

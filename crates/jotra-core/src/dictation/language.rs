//! The languages dictation offers, and the on-device locale for one.

/// One language the transcription endpoint accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// ISO-639-1 code, as the `language` field of a transcription request.
    pub code: &'static str,
    /// English name, as the Settings list shows it.
    pub name: &'static str,
}

const fn lang(code: &'static str, name: &'static str) -> Language {
    Language { code, name }
}

/// The languages OpenAI's speech-to-text guide lists, sorted by name.
pub const LANGUAGES: &[Language] = &[
    lang("af", "Afrikaans"),
    lang("ar", "Arabic"),
    lang("hy", "Armenian"),
    lang("az", "Azerbaijani"),
    lang("be", "Belarusian"),
    lang("bs", "Bosnian"),
    lang("bg", "Bulgarian"),
    lang("ca", "Catalan"),
    lang("zh", "Chinese"),
    lang("hr", "Croatian"),
    lang("cs", "Czech"),
    lang("da", "Danish"),
    lang("nl", "Dutch"),
    lang("en", "English"),
    lang("et", "Estonian"),
    lang("fi", "Finnish"),
    lang("fr", "French"),
    lang("gl", "Galician"),
    lang("de", "German"),
    lang("el", "Greek"),
    lang("he", "Hebrew"),
    lang("hi", "Hindi"),
    lang("hu", "Hungarian"),
    lang("is", "Icelandic"),
    lang("id", "Indonesian"),
    lang("it", "Italian"),
    lang("ja", "Japanese"),
    lang("kn", "Kannada"),
    lang("kk", "Kazakh"),
    lang("ko", "Korean"),
    lang("lv", "Latvian"),
    lang("lt", "Lithuanian"),
    lang("mk", "Macedonian"),
    lang("ms", "Malay"),
    lang("mi", "Maori"),
    lang("mr", "Marathi"),
    lang("ne", "Nepali"),
    lang("no", "Norwegian"),
    lang("fa", "Persian"),
    lang("pl", "Polish"),
    lang("pt", "Portuguese"),
    lang("ro", "Romanian"),
    lang("ru", "Russian"),
    lang("sr", "Serbian"),
    lang("sk", "Slovak"),
    lang("sl", "Slovenian"),
    lang("es", "Spanish"),
    lang("sw", "Swahili"),
    lang("sv", "Swedish"),
    lang("tl", "Tagalog"),
    lang("ta", "Tamil"),
    lang("th", "Thai"),
    lang("tr", "Turkish"),
    lang("uk", "Ukrainian"),
    lang("ur", "Urdu"),
    lang("vi", "Vietnamese"),
    lang("cy", "Welsh"),
];

/// The listed language with `code`.
pub fn language(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code == code)
}

/// A locale identifier's language and region subtags, lowercased and
/// uppercased. Accepts `en_GB`, `en-GB`, `zh-Hant-TW`, `en_GB@calendar=x`
/// and `en-US-u-ca-gregory`.
fn split_locale(identifier: &str) -> (String, Option<String>) {
    let base = identifier
        .split(['@', '.'])
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    let mut parts = base.split('-').filter(|p| !p.is_empty());
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    let mut region = None;
    for part in parts {
        if part.len() == 1 {
            break;
        }
        let is_region = (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
            || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()));
        if is_region {
            region = Some(part.to_ascii_uppercase());
            break;
        }
    }
    (language, region)
}

/// Whether a locale's language subtag is the listed language `code`.
fn speaks(locale_language: &str, code: &str) -> bool {
    locale_language == code || (code == "no" && locale_language == "nb")
}

/// The on-device locale for `language` from `supported`.
///
/// An empty `language` (automatic) means the system language: the on-device
/// engine cannot detect one. A locale in the system's region is preferred,
/// then the first supported locale that speaks the language. `None` when the
/// engine has none.
pub fn on_device_locale(
    language: &str,
    system_locale: &str,
    supported: &[String],
) -> Option<String> {
    let (system_language, system_region) = split_locale(system_locale);
    let wanted = if language.trim().is_empty() {
        system_language
    } else {
        language.trim().to_ascii_lowercase()
    };
    if wanted.is_empty() {
        return None;
    }
    let candidates: Vec<(&String, Option<String>)> = supported
        .iter()
        .filter_map(|id| {
            let (lang, region) = split_locale(id);
            speaks(&lang, &wanted).then_some((id, region))
        })
        .collect();
    candidates
        .iter()
        .find(|(_, region)| region.is_some() && *region == system_region)
        .or_else(|| candidates.first())
        .map(|(id, _)| (*id).clone())
}

/// The listed language codes that have at least one locale in `supported`,
/// in list order.
pub fn on_device_languages(supported: &[String]) -> Vec<String> {
    LANGUAGES
        .iter()
        .filter(|l| {
            supported
                .iter()
                .any(|id| speaks(&split_locale(id).0, l.code))
        })
        .map(|l| l.code.to_string())
        .collect()
}

/// The language subtag of a locale identifier, as `system_language` reports it.
pub fn locale_language(identifier: &str) -> String {
    split_locale(identifier).0
}

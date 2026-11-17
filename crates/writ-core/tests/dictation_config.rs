use writ_core::config::{
    effective_engine, AppId, DictationConfig, DictationEngine, DictationProvider, WritConfig,
};

#[test]
fn a_fresh_config_has_dictation_off_with_the_openai_preset() {
    let d = DictationConfig::default();
    assert!(!d.enabled);
    assert_eq!(d.engine, None);
    assert_eq!(d.provider, DictationProvider::OpenAi);
    assert_eq!(d.base_url, "");
    assert_eq!(d.model, "gpt-transcribe");
    assert_eq!(d.language, "");
    assert!(!d.clean_up);
    assert!(d.consented_hosts.is_empty());
    assert!(!WritConfig::default().is_app_on(AppId::Dictation));
}

#[test]
fn a_file_without_the_section_reads_as_dictation_off() {
    let parsed = WritConfig::parse("[editor]\nfont_size = 15\n").unwrap();
    assert_eq!(parsed.dictation, DictationConfig::default());
}

#[test]
fn the_section_round_trips_through_toml() {
    let config = WritConfig {
        dictation: DictationConfig {
            enabled: true,
            engine: Some(DictationEngine::OnDevice),
            provider: DictationProvider::Groq,
            base_url: "http://localhost:9000/v1".to_string(),
            model: "whisper-large-v3-turbo".to_string(),
            language: "ar".to_string(),
            clean_up: true,
            consented_hosts: vec!["api.groq.com".to_string()],
        },
        ..WritConfig::default()
    };
    let text = toml::to_string(&config).unwrap();
    assert!(text.contains("[dictation]"), "{text}");
    assert!(text.contains("engine = \"on_device\""), "{text}");
    assert!(text.contains("provider = \"groq\""), "{text}");
    assert_eq!(
        WritConfig::parse(&text).unwrap().dictation,
        config.dictation
    );
}

#[test]
fn an_unpicked_engine_is_not_written() {
    let text = toml::to_string(&WritConfig::default()).unwrap();
    let section = text.split("[dictation]").nth(1).unwrap();
    let section = section.split("\n[").next().unwrap();
    assert!(!section.contains("engine"), "{section}");
}

#[test]
fn every_provider_reads_from_its_config_word() {
    for (word, provider) in [
        ("openai", DictationProvider::OpenAi),
        ("groq", DictationProvider::Groq),
        ("custom", DictationProvider::Custom),
    ] {
        let parsed = WritConfig::parse(&format!("[dictation]\nprovider = \"{word}\"\n")).unwrap();
        assert_eq!(parsed.dictation.provider, provider);
        assert_eq!(provider.id(), word);
    }
}

#[test]
fn the_language_is_trimmed_and_lowercased() {
    let parsed = WritConfig::parse("[dictation]\nlanguage = \" AR \"\n").unwrap();
    assert_eq!(parsed.dictation.language, "ar");
}

#[test]
fn an_unknown_engine_is_named_and_falls_back() {
    let lenient = WritConfig::parse_lenient(
        "[dictation]\nenabled = true\nengine = \"cloud\"\nmodel = \"whisper-1\"\n",
    );
    assert_eq!(lenient.diagnostics.len(), 1, "{:?}", lenient.diagnostics);
    assert_eq!(lenient.diagnostics[0].key, "dictation.engine");
    assert_eq!(lenient.config.dictation.engine, None);
    assert!(lenient.config.dictation.enabled);
    assert_eq!(lenient.config.dictation.model, "whisper-1");
}

#[test]
fn an_unknown_provider_is_named_and_falls_back() {
    let lenient = WritConfig::parse_lenient("[dictation]\nprovider = \"azure\"\n");
    assert_eq!(lenient.diagnostics.len(), 1, "{:?}", lenient.diagnostics);
    assert_eq!(lenient.diagnostics[0].key, "dictation.provider");
    assert_eq!(lenient.config.dictation.provider, DictationProvider::OpenAi);
}

#[test]
fn an_unlisted_language_is_named_and_reads_as_automatic() {
    let lenient =
        WritConfig::parse_lenient("[dictation]\nlanguage = \"klingon\"\nclean_up = true\n");
    assert_eq!(lenient.diagnostics.len(), 1, "{:?}", lenient.diagnostics);
    assert_eq!(lenient.diagnostics[0].key, "dictation.language");
    assert!(lenient.diagnostics[0].message.contains("klingon"));
    assert_eq!(lenient.config.dictation.language, "");
    assert!(lenient.config.dictation.clean_up);
}

#[test]
fn presets_send_to_their_provider_rows_and_custom_to_the_typed_url() {
    let mut d = DictationConfig {
        base_url: "http://192.168.1.20:8000/v1/".to_string(),
        ..DictationConfig::default()
    };
    assert_eq!(d.effective_base_url(), "https://api.openai.com/v1");
    d.provider = DictationProvider::Groq;
    assert_eq!(d.effective_base_url(), "https://api.groq.com/openai/v1");
    d.provider = DictationProvider::Custom;
    assert_eq!(d.effective_base_url(), "http://192.168.1.20:8000/v1/");
    d.base_url = "  ".to_string();
    assert_eq!(d.effective_base_url(), "");
}

#[test]
fn an_unpicked_engine_is_on_device_where_supported_and_api_elsewhere() {
    assert_eq!(effective_engine(None, true), DictationEngine::OnDevice);
    assert_eq!(effective_engine(None, false), DictationEngine::Api);
    assert_eq!(
        effective_engine(Some(DictationEngine::Api), true),
        DictationEngine::Api
    );
    assert_eq!(
        effective_engine(Some(DictationEngine::OnDevice), false),
        DictationEngine::OnDevice
    );
    let d = DictationConfig {
        engine: Some(DictationEngine::Api),
        ..DictationConfig::default()
    };
    assert_eq!(d.effective_engine(true), DictationEngine::Api);
    assert_eq!(
        DictationConfig::default().effective_engine(true),
        DictationEngine::OnDevice
    );
}

#[test]
fn the_app_switch_is_the_sections_enabled_key() {
    let mut config = WritConfig::default();
    config.set_app(AppId::Dictation, true);
    assert!(config.dictation.enabled);
    assert!(config.is_app_on(AppId::Dictation));
    config.set_app(AppId::Dictation, false);
    assert!(!config.dictation.enabled);
    assert!(!config.is_app_on(AppId::Dictation));
    assert_eq!(AppId::ALL.last(), Some(&AppId::Dictation));
}

#[test]
fn a_stale_copy_keeps_the_audio_consents_rust_wrote() {
    let mut live = WritConfig::default();
    live.dictation.consented_hosts = vec!["api.openai.com".to_string()];
    let mut stale = WritConfig::default();
    stale.dictation.enabled = true;
    stale.carry_rust_owned(&live);
    assert_eq!(stale.dictation.consented_hosts, ["api.openai.com"]);
    assert!(stale.dictation.enabled);
    assert!(stale.ai.consented_hosts.is_empty());
}

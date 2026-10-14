//! The refusals that send a person to a setting name where it is in Settings.

use writ_core::chat::ChatError;
use writ_core::polish::PolishError;

#[test]
fn a_missing_model_or_provider_names_the_settings_page_that_holds_it() {
    assert_eq!(
        PolishError::ModelRequired.to_string(),
        "Choose a model in Settings, Apps, Rewrite."
    );
    assert_eq!(
        ChatError::ModelRequired.to_string(),
        "Choose a chat model in Settings, Apps, Chat."
    );
    assert_eq!(
        ChatError::UnknownProvider("nope".into()).to_string(),
        "Choose a provider in Settings, Apps, Chat."
    );
    for message in [
        PolishError::ModelRequired.to_string(),
        ChatError::ModelRequired.to_string(),
    ] {
        assert!(!message.contains("AI settings"), "{message}");
    }
}

use super::*;

fn rule(pattern: &str) -> TextHighlightRule {
    TextHighlightRule {
        pattern: pattern.into(),
        ..Default::default()
    }
}

#[test]
fn isolates_invalid_rules_from_valid_configuration() {
    let compiled = compile_configuration(
        true,
        &[rule("ERROR"), rule("(")],
        &PromptHeaderConfig::default(),
    );
    assert_eq!(compiled.matcher.rules.len(), 1);
    assert_eq!(compiled.errors.len(), 1);
}

#[test]
fn invalid_prompt_regex_preserves_metadata_style() {
    let compiled = compile_configuration(
        false,
        &[],
        &PromptHeaderConfig {
            enabled: true,
            fallback_regex: Some("(".into()),
            ..Default::default()
        },
    );
    assert!(compiled.header_style.is_some());
    assert!(compiled.matcher.prompt_header.is_none());
}

#[test]
fn disabled_highlighting_preserves_prompt_configuration() {
    let compiled = compile_configuration(
        false,
        &[rule("ERROR")],
        &PromptHeaderConfig {
            enabled: true,
            fallback_regex: Some("^prompt> ".into()),
            ..Default::default()
        },
    );
    assert!(compiled.matcher.rules.is_empty());
    assert!(compiled.matcher.prompt_header.is_some());
}

#[test]
fn settings_file_round_trip_preserves_prompt_space() {
    use settings_value::SettingsValue;
    let expected = PromptHeaderConfig {
        fallback_regex: Some("^prompt> ".into()),
        ..Default::default()
    };
    assert_eq!(
        PromptHeaderConfig::from_file_value(&expected.to_file_value()),
        Some(expected)
    );
}

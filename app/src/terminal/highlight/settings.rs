use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use settings::macros::define_settings_group;
use settings::{SupportedPlatforms, SyncToCloud};
use uuid::Uuid;
use warp_terminal::model::highlight::{
    HighlightColor, HighlightMatcher, HighlightRule, HighlightStyle, HighlightTarget, MAX_RULES,
    PromptHeaderRule, compile_pattern,
};

use crate::features::FeatureFlag;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TextHighlightRule {
    pub id: String,
    pub name: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub pattern: String,
    #[serde(default)]
    pub case_sensitive: bool,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

fn enabled_by_default() -> bool {
    true
}

impl settings_value::SettingsValue for TextHighlightRule {}

impl Default for TextHighlightRule {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: "New rule".into(),
            enabled: true,
            pattern: String::new(),
            case_sensitive: false,
            target: "text".into(),
            foreground: Some("ansi:bright_red".into()),
            background: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default)]
pub struct PromptHeaderConfig {
    pub enabled: bool,
    pub background: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    pub show_separator: bool,
    pub separator: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_regex: Option<String>,
}

impl Default for PromptHeaderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            background: "theme:surface_2".into(),
            foreground: None,
            show_separator: true,
            separator: "theme:outline".into(),
            fallback_regex: None,
        }
    }
}

impl settings_value::SettingsValue for PromptHeaderConfig {}

define_settings_group!(HighlightSettings, settings: [
    enabled: TextHighlightsEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "terminal.highlights.enabled",
        description: "Apply custom regex highlighting rules to terminal output.",
        feature_flag: FeatureFlag::TerminalHighlighting,
    },
    rules: TextHighlightRules {
        type: Vec<TextHighlightRule>,
        default: Vec::new(),
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "terminal.highlights.rules",
        description: "Ordered regex highlighting rules. The first matching rule has priority.",
        feature_flag: FeatureFlag::TerminalHighlighting,
    },
    prompt_header: PromptHeaderColors {
        type: PromptHeaderConfig,
        default: PromptHeaderConfig::default(),
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "terminal.prompt_header",
        description: "Prompt header colors and an optional regex for sessions without shell integration.",
        feature_flag: FeatureFlag::TerminalHighlighting,
    },
]);

pub struct CompiledHighlights {
    pub matcher: HighlightMatcher,
    pub header_style: Option<HighlightStyle>,
    pub header_separator: Option<HighlightColor>,
    pub errors: Vec<String>,
}

fn parse_optional_color(
    value: &Option<String>,
    allow_theme: bool,
) -> Result<Option<HighlightColor>, String> {
    value
        .as_ref()
        .map(|value| {
            HighlightColor::parse(value, allow_theme)
                .ok_or_else(|| format!("Invalid color: {value}"))
        })
        .transpose()
}

impl TextHighlightRule {
    pub fn compile(&self) -> Result<HighlightRule, String> {
        Uuid::parse_str(&self.id).map_err(|_| "Invalid rule ID.".to_string())?;
        if self.name.trim().is_empty() {
            return Err("Enter a rule name.".into());
        }
        let target = match self.target.as_str() {
            "text" => HighlightTarget::Text,
            "line" => HighlightTarget::Line,
            _ => return Err("Choose Text or Line.".into()),
        };
        let style = HighlightStyle {
            foreground: parse_optional_color(&self.foreground, false)?,
            background: parse_optional_color(&self.background, false)?,
        };
        if style.foreground.is_none() && style.background.is_none() {
            return Err("Choose a text or background color.".into());
        }
        HighlightRule::new(&self.pattern, self.case_sensitive, target, style)
    }
}

impl PromptHeaderConfig {
    pub fn style(&self) -> Result<(HighlightStyle, Option<HighlightColor>), String> {
        let background = HighlightColor::parse(&self.background, true)
            .ok_or_else(|| "Invalid prompt background color.".to_string())?;
        let foreground = parse_optional_color(&self.foreground, true)?;
        let separator = if self.show_separator {
            Some(
                HighlightColor::parse(&self.separator, true)
                    .ok_or_else(|| "Invalid separator color.".to_string())?,
            )
        } else {
            None
        };
        Ok((
            HighlightStyle {
                foreground,
                background: Some(background),
            },
            separator,
        ))
    }

    pub fn validate(&self) -> Result<(), String> {
        self.style()?;
        if let Some(pattern) = &self.fallback_regex {
            compile_pattern(pattern, true)?;
        }
        Ok(())
    }
}

pub fn compile_configuration(
    enabled: bool,
    rules: &[TextHighlightRule],
    header: &PromptHeaderConfig,
) -> CompiledHighlights {
    let mut compiled = CompiledHighlights {
        matcher: HighlightMatcher::default(),
        header_style: None,
        header_separator: None,
        errors: Vec::new(),
    };
    let mut ids = HashSet::new();
    if rules.len() > MAX_RULES {
        compiled
            .errors
            .push("At most 64 highlight rules are supported.".into());
    }
    if enabled {
        for rule in rules.iter().take(MAX_RULES) {
            if !ids.insert(&rule.id) {
                compiled
                    .errors
                    .push(format!("{}: duplicate rule ID.", rule.name));
                continue;
            }
            match rule.compile() {
                Ok(rule_compiled) if rule.enabled => compiled.matcher.rules.push(rule_compiled),
                Ok(_) => {}
                Err(error) => compiled.errors.push(format!("{}: {error}", rule.name)),
            }
        }
    }
    if header.enabled {
        match header.style() {
            Ok((style, separator)) => {
                compiled.header_style = Some(style);
                compiled.header_separator = separator;
                if let Some(pattern) = &header.fallback_regex {
                    match compile_pattern(pattern, true) {
                        Ok(regex) => {
                            compiled.matcher.prompt_header = Some(PromptHeaderRule {
                                regex,
                                style,
                                separator,
                            })
                        }
                        Err(error) => compiled.errors.push(format!("Prompt pattern: {error}")),
                    }
                }
            }
            Err(error) => compiled.errors.push(error),
        }
    }
    compiled
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

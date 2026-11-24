//! Chinese/English strings.
//!
//! While the Tauri GUI still exists, both GUIs read the same locale files, so
//! a string cannot be fixed in one and left stale in the other.

use std::{collections::HashMap, fmt::Display, sync::OnceLock};

const ZH_CN: &str = include_str!("../../../tauri-app/src/i18n/locales/zh-CN.json");
const EN_US: &str = include_str!("../../../tauri-app/src/i18n/locales/en-US.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Chinese,
    English,
}

impl Language {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Chinese => "zh-CN",
            Self::English => "en-US",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "zh-CN" => Some(Self::Chinese),
            "en-US" => Some(Self::English),
            _ => None,
        }
    }

    /// The Tauri frontend asked the WebView for `navigator.language`. There is
    /// no such query here without another dependency, so the POSIX locale
    /// variables decide, and a host that sets none (the usual macOS/Windows
    /// GUI launch) starts in English until the user picks a language once.
    pub fn detect() -> Self {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .filter(|value| value.to_ascii_lowercase().starts_with("zh"))
            .map_or(Self::English, |_| Self::Chinese)
    }
}

fn table(language: Language) -> &'static HashMap<String, String> {
    static CHINESE: OnceLock<HashMap<String, String>> = OnceLock::new();
    static ENGLISH: OnceLock<HashMap<String, String>> = OnceLock::new();
    let (cell, source) = match language {
        Language::Chinese => (&CHINESE, ZH_CN),
        Language::English => (&ENGLISH, EN_US),
    };
    // The files are compiled in and covered by a test, so a parse failure is
    // a build defect; an empty table then shows keys instead of crashing.
    cell.get_or_init(|| serde_json::from_str(source).unwrap_or_default())
}

pub fn t(language: Language, key: &str) -> String {
    table(language)
        .get(key)
        .or_else(|| table(Language::Chinese).get(key))
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

pub fn tf(language: Language, key: &str, values: &[(&str, &dyn Display)]) -> String {
    values.iter().fold(t(language, key), |text, (name, value)| {
        text.replace(&format!("{{{{{name}}}}}"), &value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_locales_parse_and_carry_the_same_keys() {
        let chinese = table(Language::Chinese);
        let english = table(Language::English);
        assert!(chinese.len() > 50);
        let mut missing: Vec<_> = chinese
            .keys()
            .filter(|key| !english.contains_key(*key))
            .chain(english.keys().filter(|key| !chinese.contains_key(*key)))
            .collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "keys missing from one locale: {missing:?}"
        );
    }

    #[test]
    fn placeholders_are_substituted() {
        assert_eq!(
            tf(Language::English, "status.selected", &[("count", &3)]),
            "3 input(s) selected."
        );
        assert_eq!(t(Language::English, "no.such.key"), "no.such.key");
    }
}

//! Read /etc/locale.gen: the locale codes (e.g., “fr_FR”) the system can generate.
//! Their names are shown by QML, from Qt's own locale data (Qt.locale(code)), so that
//! no table of language or country names has to be kept here.
//!
//! Ref: https://wiki.archlinux.org/title/Locale

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct LocaleEntry {
    /// language_COUNTRY, with its variant when it has one (“ca_ES@valencia”)
    pub code: String,
}

/// Widely used languages, in this order: a country without a locale in its own
/// language (US, GB, CN, BE…) is named with the first of them it has
const MAIN_LANGUAGES: &[&str] = &[
    "en", "zh", "ar", "es", "pt", "fr", "ru", "de", "hi", "sw", "am", "fa", "tr", "id", "ms", "ja",
    "ko", "it", "nl",
];

/// Countries whose language is neither named like the country nor the only one, and
/// where a main language (an English locale for formats, Russian, Turkish) would win
const NATIONAL_LANGUAGES: &[(&str, &str)] =
    &[("CH", "de"), ("CY", "el"), ("DK", "da"), ("IL", "he"), ("SE", "sv"), ("UA", "uk")];

/// Countries of the available locales: (country code, locale codes joined by “,”), in
/// this order: the country's own language (it_IT for IT, its only one, or the one of
/// NATIONAL_LANGUAGES), the main languages, the others, then English (en_FJ, even when
/// locale.gen has none: Qt can still name the country with it). QML names the country
/// with the first locale Qt knows; the language pre-selected for a country is the
/// first one locale.gen has
pub fn list_countries(entries: &[LocaleEntry]) -> Vec<(String, String)> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in entries {
        let base = e.code.split('@').next().unwrap_or(&e.code);
        if let Some((_, country)) = base.split_once('_') {
            map.entry(country.to_string()).or_default().push(e.code.clone());
        }
    }
    map.into_iter()
        .map(|(country, mut codes)| {
            let language = |c: &String| c.split('_').next().unwrap_or("").to_string();
            // A single language (et_EE for EE) is the country's own
            let single = codes.iter().filter(|c| !c.contains('@')).count() == 1;
            let national = NATIONAL_LANGUAGES.iter().find(|(c, _)| *c == country).map(|(_, l)| *l);
            let rank = |c: &String| {
                let lang = language(c);
                if c.contains('@') {
                    usize::MAX // a variant is never the main language
                } else if single || lang == country.to_lowercase() || national == Some(lang.as_str()) {
                    0
                } else {
                    MAIN_LANGUAGES.iter().position(|m| *m == lang).map_or(usize::MAX - 1, |i| i + 1)
                }
            };
            codes.sort_by_key(|c| rank(c));
            let english = format!("en_{country}");
            if !codes.contains(&english) {
                let at = codes.iter().position(|c| c.contains('@')).unwrap_or(codes.len());
                codes.insert(at, english);
            }
            (country, codes.join(","))
        })
        .collect()
}

/// A line of locale.gen, commented or not: “it_IT.UTF-8 UTF-8”, “ca_ES@valencia UTF-8”.
/// Only UTF-8 locales: the others (ISO-8859, @euro) are not offered
fn parse_line(line: &str) -> Option<LocaleEntry> {
    let trimmed = line.trim().trim_start_matches('#').trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    // Exactly “name charset”: the comment lines of locale.gen are sentences
    if parts.len() != 2 || parts[1] != "UTF-8" {
        return None;
    }
    let (main, variant) = match parts[0].split_once('@') {
        Some((m, v)) => (m, Some(v)),
        None => (parts[0], None),
    };
    let base = main.split('.').next().unwrap_or(main);
    let code = match variant {
        Some(v) => format!("{base}@{v}"),
        None => base.to_string(),
    };
    is_valid_locale_code(&code).then_some(LocaleEntry { code })
}

pub fn read_available_locales(path: &str) -> std::io::Result<Vec<LocaleEntry>> {
    let content = fs::read_to_string(path)?;
    let mut entries: Vec<LocaleEntry> = content.lines().filter_map(parse_line).collect();

    entries.sort_by(|a, b| a.code.cmp(&b.code));
    entries.dedup_by(|a, b| a.code == b.code);

    Ok(entries)
}

pub const DEFAULT_LOCALE_GEN_PATH: &str = "/etc/locale.gen";

/// Reads the active locale from /etc/locale.conf
pub fn read_current_system_locale() -> Option<String> {
    let content = fs::read_to_string("/etc/locale.conf").ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("LANG=") {
            let val = trimmed.trim_start_matches("LANG=").trim_matches('"').trim_matches('\'');
            // “ca_ES.UTF-8@valencia” -> “ca_ES@valencia”
            let (main, variant) = val.split_once('@').map_or((val, None), |(m, v)| (m, Some(v)));
            let base = main.split('.').next().unwrap_or(main);
            return Some(variant.map_or(base.to_string(), |v| format!("{base}@{v}")));
        }
    }
    None
}

/// Enables the locale in /etc/locale.gen, generates the system locales, and updates /etc/locale.conf
/// A valid code looks like “fr”, ‘fr_CH’, “fil_PH” (with or without
/// “.UTF-8”), or “ca_ES@valencia”. It is validated before being inserted into the command run as root
fn is_valid_locale_code(code: &str) -> bool {
    let (code, variant) = code.split_once('@').map_or((code, None), |(c, v)| (c, Some(v)));
    if variant.is_some_and(|v| v.is_empty() || !v.chars().all(|c| c.is_ascii_lowercase())) {
        return false;
    }
    let base = code.split('.').next().unwrap_or("");
    let mut parts = base.split('_');
    let lang_ok = parts
        .next()
        .map(|l| (2..=3).contains(&l.len()) && l.chars().all(|c| c.is_ascii_lowercase()))
        .unwrap_or(false);
    let country_ok = parts
        .next()
        .map(|c| c.len() == 2 && c.chars().all(|ch| ch.is_ascii_uppercase()))
        .unwrap_or(true);
    lang_ok && country_ok && parts.next().is_none()
}

pub fn persist_locale(locale_code: &str) -> std::io::Result<()> {
    if locale_code.trim().is_empty() {
        return Ok(());
    }
    if !is_valid_locale_code(locale_code) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "code de locale invalide",
        ));
    }

    // locale.gen names “it_IT.UTF-8” but “ca_ES@valencia”; LANG is “ca_ES.UTF-8@valencia”
    let (main, variant) = locale_code.trim().split_once('@').map_or((locale_code.trim(), None), |(m, v)| (m, Some(v)));
    let base_code = main.split('.').next().unwrap_or(main);
    let target_locale = match variant {
        Some(v) => format!("{base_code}.UTF-8@{v}"),
        None => format!("{base_code}.UTF-8"),
    };
    // The name in locale.gen has “.UTF-8” or not (“it_IT.UTF-8 UTF-8”, “hi_IN UTF-8”,
    // “ca_ES@valencia UTF-8”). sed -E: “\?” would be a literal “?”
    let gen_name = format!(r"{base_code}(\.UTF-8)?{}", variant.map(|v| format!("@{v}")).unwrap_or_default());
    let sed_pattern = format!(r"s/^#\s*({}\s+UTF-8)/\1/", gen_name);

    // A single Shell script that combines all administrative steps
    let composite_script = format!(
        "sed -i -E '{}' /etc/locale.gen && locale-gen && localectl set-locale LANG={}",
        sed_pattern, target_locale
    );

    // A single `pkexec` command that runs the entire script
    let status = Command::new("pkexec")
    .args(["sh", "-c", &composite_script])
    .status()?;

    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "L'autorisation a été refusée ou l'application des locales a échoué.",
        ));
    }

    // Synchronizing the local user session (KDE Plasma) — without root privileges
    if let Ok(home) = env::var("HOME") {
        let localerc_path = std::path::Path::new(&home).join(".config/plasma-localerc");
        let plasma_content = format!(
            "[Formats]\nLANG={}\n\n[Translations]\nLANGUAGE={}\n",
            target_locale, target_locale
        );
        let _ = fs::write(localerc_path, plasma_content);
    }

    update_process_locale(&target_locale);

    Ok(())
}

pub fn update_process_locale(target_locale: &str) {
    env::remove_var("LANG");
    env::set_var("LANG", target_locale);

    if env::var("LC_ALL").is_ok() {
        env::set_var("LC_ALL", target_locale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(codes: &[&str]) -> Vec<LocaleEntry> {
        codes.iter().map(|c| LocaleEntry { code: c.to_string() }).collect()
    }

    fn order(list: &[(String, String)], country: &str) -> String {
        list.iter().find(|(c, _)| c == country).map(|(_, l)| l.clone()).unwrap_or_default()
    }

    #[test]
    fn only_locale_lines_are_read() {
        assert!(parse_line("#  and <charset> is one of the character sets listed in").is_none());
        assert!(parse_line("# Configuration file for locale-gen").is_none());
        assert!(parse_line("#de_DE@euro ISO-8859-15  ").is_none());
        assert_eq!(parse_line("#it_IT.UTF-8 UTF-8  ").unwrap().code, "it_IT");
        assert_eq!(parse_line("ca_ES@valencia UTF-8").unwrap().code, "ca_ES@valencia");
    }

    #[test]
    fn countries_are_named_with_their_main_language() {
        let list = list_countries(&entries(&[
            "chr_US", "en_US", "es_US", "cy_GB", "en_GB", "bo_CN", "zh_CN", "de_BE", "fr_BE", "nl_BE",
            "it_IT", "hif_FJ", "et_EE", "da_DK", "en_DK", "nb_NO", "nn_NO", "se_NO",
        ]));
        assert!(order(&list, "US").starts_with("en_US,"));
        assert!(order(&list, "GB").starts_with("en_GB,"));
        assert!(order(&list, "CN").starts_with("zh_CN,bo_CN,en_CN"));
        assert!(order(&list, "DK").starts_with("da_DK,"));
        assert!(order(&list, "NO").starts_with("nb_NO,"));
        assert!(order(&list, "BE").starts_with("fr_BE,"));
        assert!(order(&list, "IT").starts_with("it_IT,"));
        assert!(order(&list, "FJ").starts_with("hif_FJ,en_FJ"));
        assert!(order(&list, "EE").starts_with("et_EE,"));
    }

    #[test]
    fn variants_are_valid_codes() {
        assert!(is_valid_locale_code("sr_RS@latin"));
        assert!(is_valid_locale_code("nan_TW"));
        assert!(!is_valid_locale_code("it_IT@"));
        assert!(!is_valid_locale_code("it_IT; rm"));
    }
}

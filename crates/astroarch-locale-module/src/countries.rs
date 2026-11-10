//! Languages of a country, from https://countries.dev, as locale codes.
//!
//! The country (chosen by the user, or guessed from the IP address) is the only thing
//! sent. The API answers with ISO 639-1 and 639-2 codes ("fr"/"fra", "roh"); they are
//! matched with the locales /etc/locale.gen can generate ("fr_CH", "it_CH"). Without a
//! network, the languages are the ones locale.gen has for the country.
//!
//! The request is made with `curl`, as the Control Center's probe does: no HTTP client
//! has to be compiled in the Control Center.

use std::collections::HashMap;
use std::process::Command;
use std::sync::OnceLock;

const API: &str = "https://countries.dev/alpha/";

/// The main locale of each language ("en" -> "en_US", "sv" -> "sv_SE"), from libX11
const LOCALE_ALIAS: &str = "/usr/share/X11/locale/locale.alias";

/// A language of the answer: the tags to look for in locale.gen, best first ("fr", "fra")
pub type Language = Vec<String>;

/// The tags of a language of the API. Norwegian ("no") is "nb" or "nn" in locale.gen
fn tags_of(code1: Option<&str>, code2: Option<&str>) -> Language {
    let valid = |c: &&str| (2..=3).contains(&c.len()) && c.chars().all(|c| c.is_ascii_lowercase());
    let mut tags: Language = Vec::new();
    match code1.filter(valid) {
        Some("no") => tags.extend(["nb".to_string(), "nn".to_string(), "no".to_string()]),
        Some(c) => tags.push(c.to_string()),
        None => {}
    }
    if let Some(c) = code2.filter(valid) {
        if !tags.iter().any(|t| t == c) {
            tags.push(c.to_string());
        }
    }
    tags
}

/// The languages in the answer of the API: `{"languages":[{"iso639_1":"fr",
/// "iso639_2":"fra",...},...]}`. `None`: not such an answer (an error page, a redirection)
pub fn parse_languages(json: &str) -> Option<Vec<Language>> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let list = value.as_object()?.get("languages")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|l| l.as_object())
            .map(|l| {
                tags_of(
                    l.get("iso639_1").and_then(|c| c.as_str()),
                    l.get("iso639_2").and_then(|c| c.as_str()),
                )
            })
            .filter(|tags| !tags.is_empty())
            .collect(),
    )
}

/// Asks the API for the languages of a country (ISO 3166-1 alpha-2). `None`: no network,
/// no `curl`, a refusal or an answer that is not understood: an expected failure, the
/// caller falls back to `fallback`
pub fn fetch_languages(country: &str) -> Option<Vec<Language>> {
    if country.len() != 2 || !country.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let url = format!("{API}{country}?fields=languages");
    let out = Command::new("curl")
        .args(["-fsS", "--proto", "=https", "-m", "4", "-H", "Accept: application/json", url.as_str()])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    parse_languages(&String::from_utf8_lossy(&out.stdout))
}

/// Language -> main locale of locale.alias ("en: en_US.ISO8859-1"), first one wins
fn parse_locale_alias(content: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(locale)) = (fields.next(), fields.next()) else { continue };
        let name = name.trim_end_matches(':');
        if !(2..=3).contains(&name.len()) || !name.chars().all(|c| c.is_ascii_lowercase()) {
            continue;
        }
        let locale = locale.split(['.', '@']).next().unwrap_or("");
        if locale.starts_with(&format!("{name}_")) {
            out.entry(name.to_string()).or_insert_with(|| locale.to_string());
        }
    }
    out
}

/// The main locale of a language on this system ("en" -> "en_US"), if libX11 names one
fn main_locale(tag: &str) -> Option<String> {
    static ALIAS: OnceLock<HashMap<String, String>> = OnceLock::new();
    ALIAS
        .get_or_init(|| std::fs::read_to_string(LOCALE_ALIAS).map(|c| parse_locale_alias(&c)).unwrap_or_default())
        .get(tag)
        .cloned()
}

/// The locale of locale.gen for a language of a country: ["fr", "fra"] + CH -> "fr_CH".
/// Without the country's own, the language's main one (locale.alias: "en_US"; else
/// "fr_FR"), else the first of the language
fn locale_for(tags: &[String], country: &str, available: &[String], main: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let plain = |c: &&String| !c.contains('@');
    let language = |c: &String| c.split('_').next().unwrap_or("").to_string();
    for tag in tags {
        let exact = format!("{tag}_{country}");
        if available.iter().any(|c| *c == exact) {
            return Some(exact);
        }
    }
    for tag in tags {
        if let Some(home) = main(tag).filter(|m| available.contains(m)) {
            return Some(home);
        }
        let home = format!("{tag}_{}", tag.to_uppercase());
        if available.iter().any(|c| *c == home) {
            return Some(home);
        }
        if let Some(any) = available.iter().filter(plain).find(|c| language(c) == *tag) {
            return Some(any.clone());
        }
    }
    None
}

/// Position of a locale in `order` (the country's locales, main language first)
fn rank(code: &str, order: &[String]) -> usize {
    order.iter().position(|c| c == code).unwrap_or(usize::MAX)
}

/// Locales to offer for the languages of the API: the ones locale.gen can generate,
/// without doubles, main language first
pub fn proposals(country: &str, api: &[Language], available: &[String], order: &[String]) -> Vec<String> {
    proposals_with(country, api, available, order, &main_locale)
}

fn proposals_with(
    country: &str,
    api: &[Language],
    available: &[String],
    order: &[String],
    main: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for tags in api {
        if let Some(locale) = locale_for(tags, country, available, main) {
            if !found.contains(&locale) {
                found.push(locale);
            }
        }
    }
    found.sort_by(|a, b| rank(a, order).cmp(&rank(b, order)).then(a.cmp(b)));
    found
}

/// Without the API: the locales locale.gen has for the country, main language first
pub fn fallback(country: &str, available: &[String], order: &[String]) -> Vec<String> {
    let suffix = format!("_{country}");
    let mut found: Vec<String> =
        available.iter().filter(|c| !c.contains('@') && c.ends_with(&suffix)).cloned().collect();
    found.sort_by(|a, b| rank(a, order).cmp(&rank(b, order)).then(a.cmp(b)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    /// locale.alias as libX11 has it, for the tests
    fn alias(tag: &str) -> Option<String> {
        parse_locale_alias("en: en_US.ISO8859-1\nenglish en_US.ISO8859-1\nsv\tsv_SE.ISO8859-1\nar: ar_AA.ISO8859-6\n")
            .get(tag)
            .cloned()
    }

    #[test]
    fn reads_the_answer_of_the_api() {
        let ch = r#"{"languages":[{"name":"German","iso639_1":"de","iso639_2":"deu","nativeName":"Deutsch"},
            {"name":"French","iso639_1":"fr","iso639_2":"fra"},{"name":"Romansh","iso639_2":"roh"}]}"#;
        assert_eq!(
            parse_languages(ch).unwrap(),
            vec![list(&["de", "deu"]), list(&["fr", "fra"]), list(&["roh"])]
        );
        // Norwegian: Bokmål or Nynorsk in locale.gen
        let no = r#"{"languages":[{"iso639_1":"no","iso639_2":"nor"}]}"#;
        assert_eq!(parse_languages(no).unwrap(), vec![list(&["nb", "nn", "no", "nor"])]);
        // A country without languages: a valid answer, nothing in it
        assert_eq!(parse_languages(r#"{"languages":[]}"#).unwrap(), Vec::<Language>::new());
        // Not an answer of the API: no cache of an empty list
        assert!(parse_languages("Country not found").is_none());
        assert!(parse_languages(r#"{"success":false,"errors":[{"message":"deprecated"}]}"#).is_none());
        assert!(parse_languages("<html><title>301 Moved Permanently</title></html>").is_none());
    }

    #[test]
    fn switzerland_offers_its_languages() {
        let available = list(&["de_CH", "fr_CH", "it_CH", "gsw_CH", "de_DE", "fr_FR", "en_US"]);
        let order = list(&["de_CH", "fr_CH", "it_CH", "gsw_CH"]);
        let api = vec![list(&["de", "deu"]), list(&["fr", "fra"]), list(&["it", "ita"]), list(&["roh"])];
        assert_eq!(proposals_with("CH", &api, &available, &order, &alias), list(&["de_CH", "fr_CH", "it_CH"]));
    }

    #[test]
    fn a_language_without_a_locale_of_the_country_takes_its_main_one() {
        let available = list(&["en_AG", "en_GB", "en_US", "fr_CA", "fr_FR", "lb_LU", "de_DE", "ar_AE", "sv_FI", "sv_SE"]);
        assert_eq!(locale_for(&list(&["fr", "fra"]), "LU", &available, &alias).unwrap(), "fr_FR");
        assert_eq!(locale_for(&list(&["lb", "ltz"]), "LU", &available, &alias).unwrap(), "lb_LU");
        // en_EN does not exist: the main one of locale.alias, not the first (en_AG)
        assert_eq!(locale_for(&list(&["en", "eng"]), "RW", &available, &alias).unwrap(), "en_US");
        assert_eq!(locale_for(&list(&["sv", "swe"]), "AX", &available, &alias).unwrap(), "sv_SE");
        // A main one that locale.gen does not have (ar_AA): the first of the language
        assert_eq!(locale_for(&list(&["ar", "ara"]), "TD", &available, &alias).unwrap(), "ar_AE");
        assert!(locale_for(&list(&["roh"]), "CH", &available, &alias).is_none());
    }

    #[test]
    fn norwegian_is_bokmal_or_nynorsk() {
        let available = list(&["nb_NO", "nn_NO"]);
        assert_eq!(locale_for(&list(&["nb", "nn", "no", "nor"]), "NO", &available, &alias).unwrap(), "nb_NO");
    }

    #[test]
    fn variants_are_never_a_fallback() {
        let available = list(&["ca_ES@valencia", "es_ES"]);
        assert!(locale_for(&list(&["ca", "cat"]), "AD", &available, &alias).is_none());
    }

    #[test]
    fn locale_alias_gives_the_main_locale_of_a_language() {
        let map = parse_locale_alias("# comment\nen: en_US.ISO8859-1\nen_US en_US.ISO8859-1\nja: ja_JP.eucJP\nja: ja_JP.SJIS\nenglish: en_US.ISO8859-1\nC C\n");
        assert_eq!(map.get("en").unwrap(), "en_US");
        assert_eq!(map.get("ja").unwrap(), "ja_JP");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn no_network_gives_the_locales_of_the_country() {
        let available = list(&["fr_BE", "nl_BE", "de_BE", "fr_FR", "ca_ES@valencia"]);
        let order = list(&["fr_BE", "nl_BE", "de_BE"]);
        assert_eq!(fallback("BE", &available, &order), list(&["fr_BE", "nl_BE", "de_BE"]));
    }

    #[test]
    fn the_country_is_checked_before_the_request() {
        assert!(fetch_languages("ch").is_none());
        assert!(fetch_languages("CHE").is_none());
        assert!(fetch_languages("C/").is_none());
    }
}

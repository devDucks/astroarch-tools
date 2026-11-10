//! Languages of a country, from https://restcountries.com, as locale codes.
//!
//! The country (chosen by the user, or guessed from the IP address) is the only thing
//! sent. The API answers with ISO 639-3 codes ("fra", "gsw", "ita"); they are matched
//! with the locales /etc/locale.gen can generate ("fr_CH", "gsw_CH", "it_CH"). Without a
//! network, the languages are the ones locale.gen has for the country.
//!
//! The request is made with `curl`, as the Control Center's probe does: no HTTP client
//! has to be compiled in the Control Center.

use std::process::Command;

const API: &str = "https://restcountries.com/v3.1/alpha/";

/// ISO 639-3 -> ISO 639-1 (sorted by the first code); a language without a two-letter
/// code keeps its three-letter one in locale.gen ("gsw_CH", "fil_PH")
const ISO639: &[(&str, &str)] = &[
    ("aar", "aa"), ("abk", "ab"), ("afr", "af"), ("aka", "ak"), ("amh", "am"), ("ara", "ar"),
    ("arg", "an"), ("asm", "as"), ("ava", "av"), ("ave", "ae"), ("aym", "ay"), ("aze", "az"),
    ("bak", "ba"), ("bam", "bm"), ("bel", "be"), ("ben", "bn"), ("bis", "bi"), ("bod", "bo"),
    ("bos", "bs"), ("bre", "br"), ("bul", "bg"), ("cat", "ca"), ("ces", "cs"), ("cha", "ch"),
    ("che", "ce"), ("chu", "cu"), ("chv", "cv"), ("cor", "kw"), ("cos", "co"), ("cre", "cr"),
    ("cym", "cy"), ("dan", "da"), ("deu", "de"), ("div", "dv"), ("dzo", "dz"), ("ell", "el"),
    ("eng", "en"), ("epo", "eo"), ("est", "et"), ("eus", "eu"), ("ewe", "ee"), ("fao", "fo"),
    ("fas", "fa"), ("fij", "fj"), ("fin", "fi"), ("fra", "fr"), ("fry", "fy"), ("ful", "ff"),
    ("gla", "gd"), ("gle", "ga"), ("glg", "gl"), ("glv", "gv"), ("grn", "gn"), ("guj", "gu"),
    ("hat", "ht"), ("hau", "ha"), ("hbs", "sh"), ("heb", "he"), ("her", "hz"), ("hin", "hi"),
    ("hmo", "ho"), ("hrv", "hr"), ("hun", "hu"), ("hye", "hy"), ("ibo", "ig"), ("ido", "io"),
    ("iii", "ii"), ("iku", "iu"), ("ile", "ie"), ("ina", "ia"), ("ind", "id"), ("ipk", "ik"),
    ("isl", "is"), ("ita", "it"), ("jav", "jv"), ("jpn", "ja"), ("kal", "kl"), ("kan", "kn"),
    ("kas", "ks"), ("kat", "ka"), ("kau", "kr"), ("kaz", "kk"), ("khm", "km"), ("kik", "ki"),
    ("kin", "rw"), ("kir", "ky"), ("kom", "kv"), ("kon", "kg"), ("kor", "ko"), ("kua", "kj"),
    ("kur", "ku"), ("lao", "lo"), ("lat", "la"), ("lav", "lv"), ("lim", "li"), ("lin", "ln"),
    ("lit", "lt"), ("ltz", "lb"), ("lub", "lu"), ("lug", "lg"), ("mah", "mh"), ("mal", "ml"),
    ("mar", "mr"), ("mkd", "mk"), ("mlg", "mg"), ("mlt", "mt"), ("mon", "mn"), ("mri", "mi"),
    ("msa", "ms"), ("mya", "my"), ("nau", "na"), ("nav", "nv"), ("nbl", "nr"), ("nde", "nd"),
    ("ndo", "ng"), ("nep", "ne"), ("nld", "nl"), ("nno", "nn"), ("nob", "nb"), ("nor", "no"),
    ("nya", "ny"), ("oci", "oc"), ("oji", "oj"), ("ori", "or"), ("orm", "om"), ("oss", "os"),
    ("pan", "pa"), ("pli", "pi"), ("pol", "pl"), ("por", "pt"), ("pus", "ps"), ("que", "qu"),
    ("roh", "rm"), ("ron", "ro"), ("run", "rn"), ("rus", "ru"), ("sag", "sg"), ("san", "sa"),
    ("sin", "si"), ("slk", "sk"), ("slv", "sl"), ("sme", "se"), ("smo", "sm"), ("sna", "sn"),
    ("snd", "sd"), ("som", "so"), ("sot", "st"), ("spa", "es"), ("sqi", "sq"), ("srd", "sc"),
    ("srp", "sr"), ("ssw", "ss"), ("sun", "su"), ("swa", "sw"), ("swe", "sv"), ("tah", "ty"),
    ("tam", "ta"), ("tat", "tt"), ("tel", "te"), ("tgk", "tg"), ("tgl", "tl"), ("tha", "th"),
    ("tir", "ti"), ("ton", "to"), ("tsn", "tn"), ("tso", "ts"), ("tuk", "tk"), ("tur", "tr"),
    ("twi", "tw"), ("uig", "ug"), ("ukr", "uk"), ("urd", "ur"), ("uzb", "uz"), ("ven", "ve"),
    ("vie", "vi"), ("vol", "vo"), ("wln", "wa"), ("wol", "wo"), ("xho", "xh"), ("yid", "yi"),
    ("yor", "yo"), ("zha", "za"), ("zho", "zh"), ("zul", "zu"),
];

fn iso639_1(code3: &str) -> Option<&'static str> {
    ISO639.binary_search_by_key(&code3, |(a, _)| *a).ok().map(|i| ISO639[i].1)
}

/// The language tags to look for in locale.gen, best first. "nor" is "nb" or "nn" there
fn tags(code3: &str) -> Vec<String> {
    let mut tags: Vec<String> = match code3 {
        "nor" => vec!["nb".into(), "nn".into()],
        _ => iso639_1(code3).map(|c| vec![c.to_string()]).unwrap_or_default(),
    };
    tags.push(code3.to_string());
    tags
}

/// ISO 639-3 codes of the languages in the answer of the API. The answer is an object, or
/// a list holding it
pub fn parse_languages(json: &str) -> Option<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let country = match &value {
        serde_json::Value::Array(list) => list.first()?,
        other => other,
    };
    let object = country.as_object()?;
    if object.contains_key("status") || object.contains_key("message") {
        return None;
    }
    Some(
        object
            .get("languages")
            .and_then(|l| l.as_object())
            .map(|l| l.keys().cloned().collect())
            .unwrap_or_default(),
    )
}

/// Asks the API for the languages of a country (ISO 3166-1 alpha-2). `None`: no network,
/// no `curl`, a refusal or an answer that is not understood: an expected failure, the
/// caller falls back to `fallback`
pub fn fetch_languages(country: &str) -> Option<Vec<String>> {
    if country.len() != 2 || !country.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let url = format!("{API}{country}?fields=languages");
    let out = Command::new("curl")
        .args(["-fsS", "-m", "4", "-H", "Accept: application/json", url.as_str()])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    parse_languages(&String::from_utf8_lossy(&out.stdout))
}

/// The locale of locale.gen for a language of a country: "fra" + CH -> "fr_CH". Without
/// the country's own, the language's main one ("fr_FR"), else the first of the language
fn locale_for(code3: &str, country: &str, available: &[String]) -> Option<String> {
    let plain = |c: &&String| !c.contains('@');
    let language = |c: &String| c.split('_').next().unwrap_or("").to_string();
    let tags = tags(code3);
    for tag in &tags {
        let exact = format!("{tag}_{country}");
        if available.iter().any(|c| *c == exact) {
            return Some(exact);
        }
    }
    for tag in &tags {
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
pub fn proposals(country: &str, api: &[String], available: &[String], order: &[String]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for code3 in api {
        if let Some(locale) = locale_for(code3, country, available) {
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

    #[test]
    fn reads_the_answer_of_the_api() {
        let one = r#"[{"languages":{"fra":"French","gsw":"Swiss German","ita":"Italian"}}]"#;
        assert_eq!(parse_languages(one).unwrap(), list(&["fra", "gsw", "ita"]));
        let object = r#"{"languages":{"eng":"English"}}"#;
        assert_eq!(parse_languages(object).unwrap(), list(&["eng"]));
        // A country without languages (Antarctica): a valid answer, nothing in it
        assert_eq!(parse_languages(r#"[{}]"#).unwrap(), Vec::<String>::new());
        assert!(parse_languages(r#"{"status":404,"message":"Not Found"}"#).is_none());
        assert!(parse_languages("<html>").is_none());
    }

    #[test]
    fn switzerland_offers_its_languages() {
        let available = list(&["de_CH", "fr_CH", "it_CH", "gsw_CH", "de_DE", "fr_FR", "en_US"]);
        let order = list(&["de_CH", "fr_CH", "it_CH", "gsw_CH"]);
        let api = list(&["deu", "fra", "gsw", "ita", "roh"]);
        assert_eq!(proposals("CH", &api, &available, &order), list(&["de_CH", "fr_CH", "it_CH", "gsw_CH"]));
    }

    #[test]
    fn a_language_without_a_locale_of_the_country_takes_its_main_one() {
        let available = list(&["fr_FR", "fr_CA", "en_US", "en_GB", "lb_LU", "de_DE"]);
        assert_eq!(locale_for("fra", "LU", &available).unwrap(), "fr_FR");
        assert_eq!(locale_for("ltz", "LU", &available).unwrap(), "lb_LU");
        assert_eq!(locale_for("eng", "LU", &available).unwrap(), "en_US");
        assert!(locale_for("roh", "CH", &available).is_none());
    }

    #[test]
    fn norwegian_is_bokmal_or_nynorsk() {
        let available = list(&["nb_NO", "nn_NO"]);
        assert_eq!(locale_for("nor", "NO", &available).unwrap(), "nb_NO");
    }

    #[test]
    fn variants_are_never_a_fallback() {
        let available = list(&["ca_ES@valencia", "es_ES"]);
        assert!(locale_for("cat", "AD", &available).is_none());
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

//! Read /etc/locale.gen and convert the codes (e.g., “fr_FR.UTF-8”)
//! to human-readable names (e.g., “French (France)”).
//!
//! Ref: https://wiki.archlinux.org/title/Locale

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct LocaleEntry {
    /// Raw code as it appears in locale.gen
    pub raw: String,
    /// Just the language_COUNTRY part
    pub code: String,
    /// Charset
    pub charset: String,
    /// Human-readable name
    pub display_name: String,
}

fn language_names() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("aa", "Afar"), ("af", "Afrikaans"), ("am", "Amharique"),
                  ("an", "Aragonais"), ("ar", "Arabe"), ("as", "Assamais"),
                  ("ast", "Asturien"), ("az", "Azerbaïdjanais"), ("be", "Biélorusse"),
                  ("bg", "Bulgare"), ("bn", "Bengali"), ("bo", "Tibétain"),
                  ("br", "Breton"), ("bs", "Bosniaque"), ("ca", "Catalan"),
                  ("ce", "Tchétchène"), ("ckb", "Kurde sorani"), ("cs", "Tchèque"),
                  ("csb", "Kachoube"), ("cv", "Tchouvache"), ("cy", "Gallois"),
                  ("da", "Danois"), ("de", "Allemand"), ("dv", "Divehi"),
                  ("dz", "Dzongkha"), ("el", "Grec"), ("en", "Anglais"),
                  ("eo", "Espéranto"), ("es", "Espagnol"), ("et", "Estonien"),
                  ("eu", "Basque"), ("fa", "Persan"), ("ff", "Peul"),
                  ("fi", "Finnois"), ("fil", "Filipino"), ("fo", "Féroïen"),
                  ("fr", "Français"), ("fur", "Frioulan"), ("fy", "Frison"),
                  ("ga", "Irlandais"), ("gd", "Gaélique écossais"), ("gl", "Galicien"),
                  ("gu", "Gujarati"), ("gv", "Mannois"), ("ha", "Haoussa"),
                  ("he", "Hébreu"), ("hi", "Hindi"), ("hne", "Chhattisgarhi"),
                  ("hr", "Croate"), ("hsb", "Haut-sorabe"), ("ht", "Créole haïtien"),
                  ("hu", "Hongrois"), ("hy", "Arménien"), ("ia", "Interlingua"),
                  ("id", "Indonésien"), ("ig", "Igbo"), ("ik", "Inupiaq"),
                  ("is", "Islandais"), ("it", "Italien"), ("iu", "Inuktitut"),
                  ("ja", "Japonais"), ("ka", "Géorgien"), ("kk", "Kazakh"),
                  ("kl", "Groenlandais"), ("km", "Khmer"), ("kn", "Kannada"),
                  ("ko", "Coréen"), ("ks", "Cachemiri"), ("ku", "Kurde"),
                  ("kw", "Cornique"), ("ky", "Kirghize"), ("lb", "Luxembourgeois"),
                  ("lg", "Ganda"), ("li", "Limbourgeois"), ("lo", "Lao"),
                  ("lt", "Lituanien"), ("lv", "Letton"), ("mag", "Magahi"),
                  ("mai", "Maithili"), ("mg", "Malgache"), ("mhr", "Mari des prairies"),
                  ("mi", "Maori"), ("mk", "Macédonien"), ("ml", "Malayalam"),
                  ("mn", "Mongol"), ("mr", "Marathi"), ("ms", "Malais"),
                  ("mt", "Maltais"), ("my", "Birman"), ("nan", "Min du Sud"),
                  ("nb", "Norvégien bokmål"), ("nds", "Bas-allemand"), ("ne", "Népalais"),
                  ("nl", "Néerlandais"), ("nn", "Norvégien nynorsk"), ("nr", "Ndébélé du Sud"),
                  ("nso", "Sotho du Nord"), ("oc", "Occitan"), ("om", "Oromo"),
                  ("or", "Odia"), ("os", "Ossète"), ("pa", "Pendjabi"),
                  ("pap", "Papiamento"), ("pl", "Polonais"), ("ps", "Pachto"),
                  ("pt", "Portugais"), ("quz", "Quechua"), ("ro", "Roumain"),
                  ("ru", "Russe"), ("rw", "Kinyarwanda"), ("sa", "Sanskrit"),
                  ("sat", "Santali"), ("sc", "Sarde"), ("sd", "Sindhi"),
                  ("se", "Same du Nord"), ("shs", "Shuswap"), ("si", "Cingalais"),
                  ("sk", "Slovaque"), ("sl", "Slovène"), ("so", "Somali"),
                  ("sq", "Albanais"), ("sr", "Serbe"), ("ss", "Swati"),
                  ("st", "Sotho du Sud"), ("sv", "Suédois"), ("sw", "Swahili"),
                  ("szl", "Silésien"), ("ta", "Tamoul"), ("tcy", "Tulu"),
                  ("te", "Télougou"), ("tg", "Tadjik"), ("th", "Thaï"),
                  ("ti", "Tigrigna"), ("tk", "Turkmène"), ("tl", "Tagalog"),
                  ("tn", "Tswana"), ("tr", "Turc"), ("ts", "Tsonga"),
                  ("tt", "Tatar"), ("ug", "Ouïghour"), ("uk", "Ukrainien"),
                  ("unm", "Unami"), ("ur", "Ourdou"), ("uz", "Ouzbek"),
                  ("ve", "Venda"), ("vi", "Vietnamien"), ("wa", "Wallon"),
                  ("wae", "Walser"), ("wo", "Wolof"), ("xh", "Xhosa"),
                  ("yi", "Yiddish"), ("yo", "Yoruba"), ("yue", "Cantonais"),
                  ("zh", "Chinois"), ("zu", "Zoulou"),
    ])
}

fn country_names() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("AD", "Andorre"), ("AE", "Émirats arabes unis"), ("AF", "Afghanistan"),
                  ("AL", "Albanie"), ("AM", "Arménie"), ("AR", "Argentine"),
                  ("AT", "Autriche"), ("AU", "Australie"), ("AZ", "Azerbaïdjan"),
                  ("BA", "Bosnie-Herzégovine"), ("BD", "Bangladesh"), ("BE", "Belgique"),
                  ("BG", "Bulgarie"), ("BH", "Bahreïn"), ("BO", "Bolivie"),
                  ("BR", "Brésil"), ("BT", "Bhoutan"), ("BW", "Botswana"),
                  ("BY", "Biélorussie"), ("CA", "Canada"), ("CH", "Suisse"),
                  ("CL", "Chili"), ("CN", "Chine"), ("CO", "Colombie"),
                  ("CR", "Costa Rica"), ("CU", "Cuba"), ("CY", "Chypre"),
                  ("CZ", "Tchéquie"), ("DE", "Allemagne"), ("DK", "Danemark"),
                  ("DO", "République dominicaine"), ("DZ", "Algérie"), ("EC", "Équateur"),
                  ("EE", "Estonie"), ("EG", "Égypte"), ("ES", "Espagne"),
                  ("ET", "Éthiopie"), ("FI", "Finlande"), ("FO", "Îles Féroé"),
                  ("FR", "France"), ("GB", "Royaume-Uni"), ("GE", "Géorgie"),
                  ("GL", "Groenland"), ("GR", "Grèce"), ("GT", "Guatemala"),
                  ("HK", "Hong Kong"), ("HN", "Honduras"), ("HR", "Croatie"),
                  ("HT", "Haïti"), ("HU", "Hongrie"), ("ID", "Indonésie"),
                  ("IE", "Irlande"), ("IL", "Israël"), ("IN", "Inde"),
                  ("IQ", "Irak"), ("IR", "Iran"), ("IS", "Islande"),
                  ("IT", "Italie"), ("JM", "Jamaïque"), ("JO", "Jordanie"),
                  ("JP", "Japon"), ("KE", "Kenya"), ("KG", "Kirghizistan"),
                  ("KH", "Cambodge"), ("KR", "Corée du Sud"), ("KW", "Koweït"),
                  ("KZ", "Kazakhstan"), ("LA", "Laos"), ("LB", "Liban"),
                  ("LI", "Liechtenstein"), ("LK", "Sri Lanka"), ("LT", "Lituanie"),
                  ("LU", "Luxembourg"), ("LV", "Lettonie"), ("LY", "Libye"),
                  ("MA", "Maroc"), ("MC", "Monaco"), ("MD", "Moldavie"),
                  ("ME", "Monténégro"), ("MK", "Macédoine du Nord"), ("MM", "Myanmar"),
                  ("MN", "Mongolie"), ("MT", "Malte"), ("MX", "Mexique"),
                  ("MY", "Malaisie"), ("NA", "Namibie"), ("NE", "Niger"),
                  ("NG", "Nigéria"), ("NI", "Nicaragua"), ("NL", "Pays-Bas"),
                  ("NO", "Norvège"), ("NP", "Népal"), ("NZ", "Nouvelle-Zélande"),
                  ("OM", "Oman"), ("PA", "Panama"), ("PE", "Pérou"),
                  ("PH", "Philippines"), ("PK", "Pakistan"), ("PL", "Pologne"),
                  ("PR", "Porto Rico"), ("PT", "Portugal"), ("PY", "Paraguay"),
                  ("QA", "Qatar"), ("RO", "Roumanie"), ("RS", "Serbie"),
                  ("RU", "Russie"), ("RW", "Rwanda"), ("SA", "Arabie saoudite"),
                  ("SD", "Soudan"), ("SE", "Suède"), ("SG", "Singapour"),
                  ("SI", "Slovénie"), ("SK", "Slovaquie"), ("SN", "Sénégal"),
                  ("SO", "Somalie"), ("SR", "Suriname"), ("SV", "Salvador"),
                  ("SY", "Syrie"), ("TH", "Thaïlande"), ("TJ", "Tadjikistan"),
                  ("TN", "Tunisie"), ("TR", "Turquie"), ("TT", "Trinité-et-Tobago"),
                  ("TW", "Taïwan"), ("TZ", "Tanzanie"), ("UA", "Ukraine"),
                  ("UG", "Ouganda"), ("US", "États-Unis"), ("UY", "Uruguay"),
                  ("UZ", "Ouzbékistan"), ("VE", "Venezuela"), ("VN", "Vietnam"),
                  ("YE", "Yémen"), ("ZA", "Afrique du Sud"), ("ZM", "Zambie"),
                  ("ZW", "Zimbabwe"),
    ])
}

fn language_name(lang_code: &str) -> String {
    language_names()
    .get(lang_code)
    .map(|s| s.to_string())
    .unwrap_or_else(|| lang_code.to_string())
}

fn country_name(country_code: &str) -> Option<String> {
    country_names().get(country_code).map(|s| s.to_string())
}

pub fn list_countries() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = country_names()
    .into_iter()
    .map(|(code, name)| (code.to_string(), name.to_string()))
    .collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v
}

fn parse_line(line: &str) -> Option<LocaleEntry> {
    let trimmed = line.trim().trim_start_matches('#').trim();
    if trimmed.is_empty() || trimmed.starts_with("SUPPORTED") {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let code_full = parts.next()?;
    let charset = parts.next().unwrap_or("UTF-8").to_string();

    let (code, _variant) = match code_full.split_once('@') {
        Some((c, v)) => (c.to_string(), Some(v.to_string())),
        None => (code_full.to_string(), None),
    };

    let code_no_charset = code.split('.').next().unwrap_or(&code).to_string();
    let (lang_part, country_part) = match code_no_charset.split_once('_') {
        Some((l, c)) => (l.to_string(), Some(c.to_string())),
        None => (code_no_charset.clone(), None),
    };

    let lang_readable = language_name(&lang_part);
    let display_name = match country_part.as_deref().and_then(country_name) {
        Some(country) => format!("{lang_readable} ({country})"),
        None => lang_readable,
    };

    Some(LocaleEntry {
        raw: trimmed.to_string(),
         code: code_no_charset,
         charset,
         display_name,
    })
}

pub fn read_available_locales(path: &str) -> std::io::Result<Vec<LocaleEntry>> {
    let content = fs::read_to_string(path)?;
    let mut entries: Vec<LocaleEntry> = content.lines().filter_map(parse_line).collect();

    entries.sort_by(|a, b| a.display_name.cmp(&b.display_name));
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
            let base = val.split('.').next().unwrap_or(val);
            return Some(base.to_string());
        }
    }
    None
}

/// Enables the locale in /etc/locale.gen, generates the system locales, and updates /etc/locale.conf
/// A valid code looks like “fr”, ‘fr_CH’, “fil_PH” (with or without
/// “.UTF-8”). It is validated before being inserted into the command run as root
fn is_valid_locale_code(code: &str) -> bool {
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

    let base_code = locale_code.split('.').next().unwrap_or(locale_code).trim();
    let target_locale = format!("{}.UTF-8", base_code);
    let sed_pattern = format!(r"s/^#\?\s*({}\s\+UTF-8)/\1/", target_locale);

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

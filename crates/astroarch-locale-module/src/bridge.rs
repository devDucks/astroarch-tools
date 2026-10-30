//! CXX-Qt Bridge: exposes Rust logic (locale.gen, IP geolocation, xkb) to QML

use crate::{geolocation, keyboard, locale_gen};
use std::sync::mpsc;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QStringList, language_codes)]
        #[qproperty(QString, selected_language_code)]
        #[qproperty(QStringList, country_codes)]
        // Per country: its locale codes joined by “,” (QML names the country with them)
        #[qproperty(QStringList, country_locales)]
        #[qproperty(QString, selected_country_code)]
        #[qproperty(QStringList, keyboard_layout_codes)]
        #[qproperty(QStringList, keyboard_layout_names)]
        #[qproperty(QString, selected_layout_code)]
        #[qproperty(QStringList, keyboard_variant_codes)]
        #[qproperty(QStringList, keyboard_variant_names)]
        #[qproperty(QString, selected_variant_code)]
        #[qproperty(bool, has_internet)]
        #[qproperty(bool, geo_detected)]
        #[qproperty(QString, detected_country_code)]
        type LocaleBridge = super::LocaleBridgeRust;

        // `geolocate`: guess the country from the IP address to pre-fill the choices
        #[qinvokable]
        fn initialize(self: Pin<&mut LocaleBridge>, geolocate: bool);

        #[qinvokable]
        fn set_language(self: Pin<&mut LocaleBridge>, code: &QString);

        #[qinvokable]
        fn set_country(self: Pin<&mut LocaleBridge>, code: &QString);

        #[qinvokable]
        fn set_keyboard_layout(self: Pin<&mut LocaleBridge>, code: &QString);

        #[qinvokable]
        fn set_keyboard_variant(self: Pin<&mut LocaleBridge>, code: &QString);

        #[qinvokable]
        fn restore_keyboard_if_unsaved(self: Pin<&mut LocaleBridge>);

        #[qinvokable]
        fn apply_keyboard_for_testing(self: Pin<&mut LocaleBridge>) -> bool;

        #[qinvokable]
        fn persist_configuration(self: Pin<&mut LocaleBridge>) -> bool;

        #[qinvokable]
        fn poll_geolocation(self: Pin<&mut LocaleBridge>);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QList, QString, QStringList};

fn to_qstringlist(items: &[QString]) -> QStringList {
    let mut list: QList<QString> = QList::default();
    for item in items {
        list.append_clone(item);
    }
    QStringList::from(&list)
}

pub struct LocaleBridgeRust {
    language_codes: QStringList,
    selected_language_code: QString,

    country_codes: QStringList,
    country_locales: QStringList,
    selected_country_code: QString,

    keyboard_layout_codes: QStringList,
    keyboard_layout_names: QStringList,
    selected_layout_code: QString,
    keyboard_variant_codes: QStringList,
    keyboard_variant_names: QStringList,
    selected_variant_code: QString,

    has_internet: bool,
    geo_detected: bool,
    detected_country_code: QString,

    geo_rx: Option<mpsc::Receiver<Option<geolocation::GeoInfo>>>,

    /// Locale codes of locale.gen, and per country its codes (see list_countries)
    available: Vec<String>,
    countries: Vec<(String, String)>,
}

impl Default for LocaleBridgeRust {
    fn default() -> Self {
        Self {
            language_codes: QStringList::default(),
            selected_language_code: QString::from("en_US"),
            country_codes: QStringList::default(),
            country_locales: QStringList::default(),
            selected_country_code: QString::default(),
            keyboard_layout_codes: QStringList::default(),
            keyboard_layout_names: QStringList::default(),
            selected_layout_code: QString::from("us"),
            keyboard_variant_codes: QStringList::default(),
            keyboard_variant_names: QStringList::default(),
            selected_variant_code: QString::default(),
            has_internet: false,
            geo_detected: false,
            detected_country_code: QString::default(),
            geo_rx: None,
            available: Vec::new(),
            countries: Vec::new(),
        }
    }
}

/// Language, layout and variant for a country. `main_locale`: its first locale in
/// locale.gen (see list_countries), for the countries not listed here
fn determine_defaults(country_code: &str, timezone: &str, main_locale: Option<&str>) -> (String, &'static str, &'static str) {
    let tz = timezone.to_lowercase();
    let (lang, layout, variant) = match country_code {
        "CH" => {
            if tz.contains("geneva") || tz.contains("lausanne") || tz.contains("zurich") {
                ("fr_CH", "ch", "fr")
            } else if tz.contains("ticino") || tz.contains("lugano") {
                ("it_CH", "ch", "it")
            } else {
                ("de_CH", "ch", "de")
            }
        }
        "BE" => {
            if tz.contains("wallonia") || tz.contains("brussels") {
                ("fr_BE", "be", "")
            } else {
                ("nl_BE", "be", "")
            }
        }
        "CA" => {
            if tz.contains("montreal") || tz.contains("quebec") {
                ("fr_CA", "ca", "fr-legacy")
            } else {
                ("en_CA", "ca", "eng")
            }
        }
        "FR" => ("fr_FR", "fr", "azerty"),
        "DE" => ("de_DE", "de", ""),
        "AT" => ("de_AT", "at", ""),
        "IT" => ("it_IT", "it", ""),
        "ES" => ("es_ES", "es", ""),
        "GB" => ("en_GB", "gb", ""),
        "US" => ("en_US", "us", ""),
        other => {
            let default_layout = keyboard::default_layout_for_country(other);
            (main_locale.unwrap_or("en_US"), default_layout, "")
        }
    };
    (lang.to_string(), layout, variant)
}

/// AstroArch image defaults: a guess from the location only replaces these
const IMAGE_LANGUAGE: &str = "en_US";
const IMAGE_LAYOUT: &str = "us";

impl qobject::LocaleBridge {
    pub fn initialize(mut self: Pin<&mut Self>, geolocate: bool) {
        // 1. Locales (their names come from Qt, in QML)
        let entries = locale_gen::read_available_locales(locale_gen::DEFAULT_LOCALE_GEN_PATH)
            .unwrap_or_default();
        let codes: Vec<QString> = entries.iter().map(|e| QString::from(e.code.as_str())).collect();
        self.as_mut().set_language_codes(to_qstringlist(&codes));
        self.as_mut().rust_mut().available = entries.iter().map(|e| e.code.clone()).collect();

        // Restores the locale already stored in the system, if one exists
        if let Some(existing_locale) = locale_gen::read_current_system_locale() {
            self.as_mut().set_selected_language_code(QString::from(existing_locale.as_str()));
        }

        // 2. Countries of these locales
        let countries = locale_gen::list_countries(&entries);
        let ccodes: Vec<QString> = countries.iter().map(|(c, _)| QString::from(c.as_str())).collect();
        let clocales: Vec<QString> = countries.iter().map(|(_, l)| QString::from(l.as_str())).collect();
        self.as_mut().set_country_codes(to_qstringlist(&ccodes));
        self.as_mut().set_country_locales(to_qstringlist(&clocales));
        self.as_mut().rust_mut().countries = countries;

        // 3. XKB Keyboards
        if let Ok(layouts) = keyboard::list_layouts() {
            let mut codes: Vec<QString> = Vec::with_capacity(layouts.len());
            let mut names: Vec<QString> = Vec::with_capacity(layouts.len());
            for l in &layouts {
                codes.push(QString::from(l.code.as_str()));
                names.push(QString::from(l.description.as_str()));
            }
            self.as_mut().set_keyboard_layout_codes(to_qstringlist(&codes));
            self.as_mut().set_keyboard_layout_names(to_qstringlist(&names));
        }

        // The layout in use now, not a default one: "Finish" keeps it unless it is changed
        if let Some((layout, variant)) = keyboard::current_layout() {
            self.as_mut().set_keyboard_layout(&QString::from(layout.as_str()));
            self.as_mut().set_selected_variant_code(QString::from(variant.as_str()));
        }

        // 4. IP Detection
        if !geolocate {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.as_mut().rust_mut().geo_rx = Some(rx);

        // Without the "geoip" feature (the AstroArch Control Center, which finds the country
        // with its own probe) there is no lookup: as without a network
        #[cfg(not(feature = "geoip"))]
        let _ = tx.send(None);
        #[cfg(feature = "geoip")]
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                           Err(_) => {
                               let _ = tx.send(None);
                               return;
                           }
            };
            rt.block_on(async {
                if !geolocation::has_internet().await {
                    let _ = tx.send(None);
                    return;
                }
                let geo = geolocation::detect_geolocation().await;
                let _ = tx.send(geo);
            });
        });
    }

    pub fn poll_geolocation(mut self: Pin<&mut Self>) {
        let result = self
        .as_mut()
        .rust_mut()
        .geo_rx
        .as_ref()
        .and_then(|rx| rx.try_recv().ok());

        let Some(maybe_geo) = result else { return };
        self.as_mut().rust_mut().geo_rx = None;

        match maybe_geo {
            Some(geo) => {
                let country_code = geo.country_code.to_uppercase();
                let timezone = geo.timezone;

                self.as_mut().set_has_internet(true);
                self.as_mut().set_geo_detected(true);

                let q_country = QString::from(country_code.as_str());
                self.as_mut().set_detected_country_code(q_country.clone());

                // A guess: it pre-selects the country, and the language and layout only
                // where they are still the image defaults (what the user set stays).
                // Choices only: nothing is applied until "Finish"
                if self.selected_country_code().to_string().is_empty() {
                    self.as_mut().set_selected_country_code(q_country);
                    let language = self.selected_language_code().to_string() == IMAGE_LANGUAGE;
                    let layout = self.selected_layout_code().to_string() == IMAGE_LAYOUT
                        && self.selected_variant_code().to_string().is_empty();
                    self.as_mut().choose_for_country(&country_code, &timezone, language, layout);
                }
            }
            None => {
                self.as_mut().set_has_internet(false);
                self.as_mut().set_geo_detected(true);
            }
        }
    }

    pub fn set_language(mut self: Pin<&mut Self>, code: &QString) {
        self.as_mut().set_selected_language_code(code.clone());
    }

    pub fn set_country(mut self: Pin<&mut Self>, code: &QString) {
        let country_str = code.to_string();
        self.as_mut().set_selected_country_code(code.clone());
        self.as_mut().choose_for_country(&country_str, "", true, true);
    }

    /// Selects the language and/or layout of a country
    fn choose_for_country(mut self: Pin<&mut Self>, country: &str, timezone: &str, language: bool, layout: bool) {
        // Its first locale that locale.gen has
        let main_locale = self
            .rust()
            .countries
            .iter()
            .find(|(c, _)| c == country)
            .and_then(|(_, codes)| codes.split(',').find(|c| self.rust().available.iter().any(|a| a == c)))
            .map(str::to_string);
        let (lang_code, layout_code, variant_code) = determine_defaults(country, timezone, main_locale.as_deref());
        // Picked by hand (no time zone): the language the country is named with
        let lang_code = if timezone.is_empty() { main_locale.unwrap_or(lang_code) } else { lang_code };

        if language {
            self.as_mut().set_selected_language_code(QString::from(lang_code.as_str()));
        }
        if layout {
            self.as_mut().set_keyboard_layout(&QString::from(layout_code));
            if !variant_code.is_empty() {
                self.as_mut().set_keyboard_variant(&QString::from(variant_code));
            }
        }
    }

    pub fn set_keyboard_layout(mut self: Pin<&mut Self>, code: &QString) {
        self.as_mut().set_selected_layout_code(code.clone());
        // A variant belongs to one layout: forget the previous one (e.g. "fr" does not exist for "us")
        self.as_mut().set_selected_variant_code(QString::from(""));
        if let Ok(variants) = keyboard::list_variants(&code.to_string()) {
            let mut codes: Vec<QString> = Vec::with_capacity(variants.len());
            let mut names: Vec<QString> = Vec::with_capacity(variants.len());
            for v in &variants {
                codes.push(QString::from(v.code.as_str()));
                names.push(QString::from(v.description.as_str()));
            }
            self.as_mut().set_keyboard_variant_codes(to_qstringlist(&codes));
            self.as_mut().set_keyboard_variant_names(to_qstringlist(&names));
        }
    }

    pub fn set_keyboard_variant(mut self: Pin<&mut Self>, code: &QString) {
        self.as_mut().set_selected_variant_code(code.clone());
    }

    pub fn restore_keyboard_if_unsaved(self: Pin<&mut Self>) {
        keyboard::restore_live_layout();
    }

    pub fn apply_keyboard_for_testing(self: Pin<&mut Self>) -> bool {
        let layout = self.selected_layout_code().to_string();
        let variant = self.selected_variant_code().to_string();
        let variant_opt = if variant.trim().is_empty() { None } else { Some(variant.as_str()) };
        keyboard::apply_layout_live(&layout, variant_opt).is_ok()
    }

    pub fn persist_configuration(self: Pin<&mut Self>) -> bool {
        let layout = self.selected_layout_code().to_string();
        let variant_str = self.selected_variant_code().to_string();
        let variant_opt = if variant_str.trim().is_empty() { None } else { Some(variant_str.as_str()) };

        let lang_code = self.selected_language_code().to_string();

        println!("--> Attempt to register : Langue = {}, Layout = {}", lang_code, layout);

        // 1. The language first (pkexec): when it is refused or fails, nothing else
        //    changes, not even the keyboard of the session
        let locale_ok = match locale_gen::persist_locale(&lang_code) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("Error saving locale settings : {:?}", e);
                false
            }
        };
        if !locale_ok {
            println!("--> Result: Locale = false, keyboard left unchanged");
            return false;
        }

        // 2. The keyboard: the system's (localectl), then, once written, the session's
        let kb_ok = match keyboard::persist_layout(&layout, variant_opt) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("Error during keyboard persistence : {:?}", e);
                false
            }
        };

        println!("--> Result: Keyboard = {}, Locale = {}", kb_ok, locale_ok);

        kb_ok
    }
}

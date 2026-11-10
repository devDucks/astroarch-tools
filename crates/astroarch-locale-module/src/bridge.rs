//! CXX-Qt Bridge: exposes Rust logic (locale.gen, IP geolocation, xkb) to QML

use crate::{countries, geolocation, keyboard, locale_gen};
use std::collections::HashMap;
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
        // Locale codes of the languages of the selected country (countries.dev, else
        // locale.gen), main first; `languages_source`: "none", "loading", "online" or "offline"
        #[qproperty(QStringList, country_language_codes)]
        #[qproperty(QString, languages_source)]
        // Keyboards proposed for the country and the language: layout i goes with variant i
        #[qproperty(QStringList, keyboard_suggestion_layouts)]
        #[qproperty(QStringList, keyboard_suggestion_variants)]
        #[qproperty(QStringList, keyboard_suggestion_names)]
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

        // One of the proposed keyboards (or any layout + variant)
        #[qinvokable]
        fn choose_keyboard(self: Pin<&mut LocaleBridge>, layout: &QString, variant: &QString);

        // Reads the answer of the languages request, if it came (polled by a QML timer)
        #[qinvokable]
        fn poll_country_languages(self: Pin<&mut LocaleBridge>);

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

    country_language_codes: QStringList,
    languages_source: QString,
    keyboard_suggestion_layouts: QStringList,
    keyboard_suggestion_variants: QStringList,
    keyboard_suggestion_names: QStringList,

    has_internet: bool,
    geo_detected: bool,
    detected_country_code: QString,

    geo_rx: Option<mpsc::Receiver<Option<geolocation::GeoInfo>>>,
    /// The request for the languages of the selected country, and the answers already had
    lang_rx: Option<mpsc::Receiver<Option<Vec<countries::Language>>>>,
    lang_cache: HashMap<String, Vec<countries::Language>>,
    /// The keyboard follows the country and the language (their first proposal is
    /// selected) until the user chooses one: what the user chose is never replaced
    keyboard_follows: bool,
    /// Layouts of base.lst
    layouts_known: Vec<String>,

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
            country_language_codes: QStringList::default(),
            languages_source: QString::from("none"),
            keyboard_suggestion_layouts: QStringList::default(),
            keyboard_suggestion_variants: QStringList::default(),
            keyboard_suggestion_names: QStringList::default(),
            has_internet: false,
            geo_detected: false,
            detected_country_code: QString::default(),
            geo_rx: None,
            lang_rx: None,
            lang_cache: HashMap::new(),
            keyboard_follows: false,
            layouts_known: Vec::new(),
            available: Vec::new(),
            countries: Vec::new(),
        }
    }
}

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
            self.as_mut().rust_mut().layouts_known = layouts.iter().map(|l| l.code.clone()).collect();
            self.as_mut().set_keyboard_layout_codes(to_qstringlist(&codes));
            self.as_mut().set_keyboard_layout_names(to_qstringlist(&names));
        }

        // The layout in use now, not a default one: "Finish" keeps it unless it is changed
        if let Some((layout, variant)) = keyboard::current_layout() {
            self.as_mut().select_layout(&layout, &variant);
        }
        // What the keyboard page can propose for the language in use (nothing is selected)
        self.as_mut().refresh_keyboard_suggestions();

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

                self.as_mut().set_has_internet(true);
                self.as_mut().set_geo_detected(true);

                let q_country = QString::from(country_code.as_str());
                self.as_mut().set_detected_country_code(q_country.clone());

                // A proposal: it pre-selects the country when the user has not chosen one.
                // Its languages and keyboards are then proposed too; nothing is applied
                // until "Finish"
                if self.selected_country_code().to_string().is_empty() {
                    self.as_mut().set_selected_country_code(q_country);
                    self.as_mut().country_chosen(&country_code);
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
        // The keyboard proposals depend on the language
        self.as_mut().rust_mut().keyboard_follows = true;
        self.as_mut().refresh_keyboard_suggestions();
    }

    pub fn set_country(mut self: Pin<&mut Self>, code: &QString) {
        let country = code.to_string();
        self.as_mut().set_selected_country_code(code.clone());
        self.as_mut().country_chosen(&country);
    }

    /// A country is chosen, by the user or from the IP address: asks for its languages
    fn country_chosen(mut self: Pin<&mut Self>, country: &str) {
        self.as_mut().rust_mut().keyboard_follows = true;
        self.as_mut().start_country_languages(country);
    }

    /// The languages of a country: from the answers already had, else asked of
    /// countries.dev in a thread (the answer is read by `poll_country_languages`)
    fn start_country_languages(mut self: Pin<&mut Self>, country: &str) {
        self.as_mut().rust_mut().lang_rx = None;
        let country = country.to_uppercase();
        if country.is_empty() {
            self.as_mut().set_country_language_codes(QStringList::default());
            self.as_mut().set_languages_source(QString::from("none"));
            return;
        }
        let known = self.rust().lang_cache.get(&country).cloned();
        if let Some(api) = known {
            self.as_mut().show_country_languages(&country, Some(api));
            return;
        }
        // Nothing is shown until the answer (or the failure) comes: no list that changes
        self.as_mut().set_country_language_codes(QStringList::default());
        self.as_mut().set_languages_source(QString::from("loading"));
        let (tx, rx) = mpsc::channel();
        self.as_mut().rust_mut().lang_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(countries::fetch_languages(&country));
        });
    }

    pub fn poll_country_languages(mut self: Pin<&mut Self>) {
        let result = self
            .as_mut()
            .rust_mut()
            .lang_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        let Some(api) = result else { return };
        self.as_mut().rust_mut().lang_rx = None;

        let country = self.selected_country_code().to_string().to_uppercase();
        if let Some(list) = &api {
            self.as_mut().rust_mut().lang_cache.insert(country.clone(), list.clone());
        }
        self.as_mut().show_country_languages(&country, api);
    }

    /// Offers the languages of the country: the ones of the API, else (no network) the ones
    /// locale.gen has for it. A single language is selected; with several, the user chooses
    fn show_country_languages(mut self: Pin<&mut Self>, country: &str, api: Option<Vec<countries::Language>>) {
        let available = self.rust().available.clone();
        // The country's locales, its own language first
        let order: Vec<String> = self
            .rust()
            .countries
            .iter()
            .find(|(c, _)| c == country)
            .map(|(_, l)| l.split(',').map(str::to_string).collect())
            .unwrap_or_default();

        let online = api.is_some();
        let mut codes = api
            .map(|api| countries::proposals(country, &api, &available, &order))
            .unwrap_or_default();
        if codes.is_empty() {
            codes = countries::fallback(country, &available, &order);
        }

        let q_codes: Vec<QString> = codes.iter().map(|c| QString::from(c.as_str())).collect();
        self.as_mut().set_country_language_codes(to_qstringlist(&q_codes));
        self.as_mut().set_languages_source(QString::from(if online { "online" } else { "offline" }));

        if codes.len() == 1 {
            self.as_mut().set_selected_language_code(QString::from(codes[0].as_str()));
        }
        self.as_mut().refresh_keyboard_suggestions();
    }

    /// The keyboards proposed for the country and the language. Until the user has chosen
    /// one, the first is the selected layout
    fn refresh_keyboard_suggestions(mut self: Pin<&mut Self>) {
        let country = self.selected_country_code().to_string();
        let language = self.selected_language_code().to_string();
        let layouts = self.rust().layouts_known.clone();
        let proposals = keyboard::suggest(&country, &language, &layouts, &|layout: &str| {
            keyboard::list_variants(layout)
                .map(|v| v.into_iter().map(|v| v.code).collect())
                .unwrap_or_default()
        });

        let q_layouts: Vec<QString> = proposals.iter().map(|p| QString::from(p.layout.as_str())).collect();
        let q_variants: Vec<QString> = proposals.iter().map(|p| QString::from(p.variant.as_str())).collect();
        self.as_mut().set_keyboard_suggestion_layouts(to_qstringlist(&q_layouts));
        self.as_mut().set_keyboard_suggestion_variants(to_qstringlist(&q_variants));
        let q_names: Vec<QString> = proposals
            .iter()
            .map(|p| QString::from(keyboard::describe(&p.layout, &p.variant).as_str()))
            .collect();
        self.as_mut().set_keyboard_suggestion_names(to_qstringlist(&q_names));

        if self.rust().keyboard_follows {
            if let Some(first) = proposals.first() {
                self.as_mut().select_layout(&first.layout, &first.variant);
            }
        }
    }

    /// Selects a layout and its variant, with the variants this layout has
    fn select_layout(mut self: Pin<&mut Self>, layout: &str, variant: &str) {
        self.as_mut().set_selected_layout_code(QString::from(layout));
        if let Ok(variants) = keyboard::list_variants(layout) {
            let mut codes: Vec<QString> = Vec::with_capacity(variants.len());
            let mut names: Vec<QString> = Vec::with_capacity(variants.len());
            for v in &variants {
                codes.push(QString::from(v.code.as_str()));
                names.push(QString::from(v.description.as_str()));
            }
            self.as_mut().set_keyboard_variant_codes(to_qstringlist(&codes));
            self.as_mut().set_keyboard_variant_names(to_qstringlist(&names));
        }
        // A variant belongs to one layout (e.g. "fr" does not exist for "us")
        self.as_mut().set_selected_variant_code(QString::from(variant));
    }

    pub fn set_keyboard_layout(mut self: Pin<&mut Self>, code: &QString) {
        self.as_mut().rust_mut().keyboard_follows = false;
        self.as_mut().select_layout(&code.to_string(), "");
    }

    pub fn set_keyboard_variant(mut self: Pin<&mut Self>, code: &QString) {
        self.as_mut().rust_mut().keyboard_follows = false;
        self.as_mut().set_selected_variant_code(code.clone());
    }

    pub fn choose_keyboard(mut self: Pin<&mut Self>, layout: &QString, variant: &QString) {
        self.as_mut().rust_mut().keyboard_follows = false;
        self.as_mut().select_layout(&layout.to_string(), &variant.to_string());
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

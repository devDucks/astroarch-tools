//! `TranslatorManager`: a QTranslator written in Rust (a QML object whose
//! base C++ class is QTranslator). It loads `:/i18n/astroarch-software-installer_<language>.qm`
//! (compiled by build.rs) based on the system language, then installs itself on the application

use core::pin::Pin;
use cxx_qt::casting::Upcast;
use cxx_qt_lib::QString;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!(<QtCore/QTranslator>);
        type QTranslator;

        include!(<QtCore/QCoreApplication>);
        type QCoreApplication;

        /// QCoreApplication::installTranslator(QTranslator *)
        #[Self = "QCoreApplication"]
        #[rust_name = "install_translator"]
        unsafe fn installTranslator(translator: *mut QTranslator) -> bool;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QTranslator]
        type TranslatorManager = super::TranslatorManagerRust;
    }

    unsafe extern "RustQt" {
        /// QTranslator::load(filename, directory, search_delimiters, suffix)
        #[inherit]
        fn load(
            self: Pin<&mut TranslatorManager>,
            filename: &QString,
            directory: &QString,
            search_delimiters: &QString,
            suffix: &QString,
        ) -> bool;

        /// Loads and installs the system translation. Returns true if a language was found
        #[qinvokable]
        fn install(self: Pin<&mut TranslatorManager>) -> bool;
    }
}

#[derive(Default)]
pub struct TranslatorManagerRust {}

/// Languages requested by the system
fn locale_candidates() -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    if let Ok(v) = std::env::var("LANGUAGE") {
        raw.extend(v.split(':').map(str::to_string));
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            raw.push(v);
        }
    }

    let mut out: Vec<String> = Vec::new();
    for item in raw {
        // "fr_FR.UTF-8@euro" -> "fr_FR"
        let base = item
            .split(|c| c == '.' || c == '@')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if base.is_empty() || base == "C" || base == "POSIX" || out.contains(&base) {
            continue;
        }
        out.push(base);
    }
    out
}

impl qobject::TranslatorManager {
    fn install(mut self: Pin<&mut Self>) -> bool {
        let directory = QString::from(":/i18n");
        // NULL strings (not empty strings): Qt then uses its default values
        let null = QString::default();

        for locale in locale_candidates() {
            let file = QString::from(format!("astroarch-software-installer_{locale}").as_str());
            if self.as_mut().load(&file, &directory, &null, &null) {
                // SAFETY: TranslatorManager does indeed inherit from QTranslator (#[base])
                let ptr = unsafe {
                    <qobject::TranslatorManager as Upcast<qobject::QTranslator>>::upcast_ptr(&*self)
                }
                .cast_mut();
                return unsafe { qobject::QCoreApplication::install_translator(ptr) };
            }
        }
        false
    }
}

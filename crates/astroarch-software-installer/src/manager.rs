
use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;

use crate::backend;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, busy)]
        type PackageManager = super::PackageManagerRust;
    }

    unsafe extern "RustQt" {
        /// Reads the TOML file and the pacman status (asynchronously)
        #[qinvokable]
        fn reload(self: Pin<&mut PackageManager>);

        /// Installs (`install = true`) or uninstalls a package (asynchronously).
        /// `aur = true`: installation using AUR
        #[qinvokable]
        fn apply(self: Pin<&mut PackageManager>, name: &QString, install: bool, aur: bool);

        /// The list is ready: a JSON array
        #[qsignal]
        #[cxx_name = "packagesLoaded"]
        fn packages_loaded(self: Pin<&mut PackageManager>, json: QString);

        /// Unable to read the list
        #[qsignal]
        #[cxx_name = "loadFailed"]
        fn load_failed(self: Pin<&mut PackageManager>, message: QString);

        /// End of an installation / uninstallation.
        /// `error_kind`: “” (success), ‘auth’ (authentication denied), or “other”
        #[qsignal]
        #[cxx_name = "operationFinished"]
        fn operation_finished(
            self: Pin<&mut PackageManager>,
            name: QString,
            ok: bool,
            installed: bool,
            error_kind: QString,
            detail: QString,
        );
    }

    impl cxx_qt::Threading for PackageManager {}
}

#[derive(Default)]
pub struct PackageManagerRust {
    busy: bool,
}

impl qobject::PackageManager {
    fn reload(mut self: Pin<&mut Self>) {
        if *self.busy() {
            return;
        }
        self.as_mut().set_busy(true);

        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = backend::load_items();
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().set_busy(false);
                match result {
                    Ok(json) => qobject
                        .as_mut()
                        .packages_loaded(QString::from(json.as_str())),
                    Err(msg) => qobject
                        .as_mut()
                        .load_failed(QString::from(msg.as_str())),
                }
            });
        });
    }

    fn apply(mut self: Pin<&mut Self>, name: &QString, install: bool, aur: bool) {
        if *self.busy() {
            return;
        }
        self.as_mut().set_busy(true);

        let name = name.to_string();
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let outcome = backend::change(&name, install, aur);
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().set_busy(false);
                qobject.as_mut().operation_finished(
                    QString::from(name.as_str()),
                    outcome.ok,
                    outcome.installed,
                    QString::from(outcome.kind.as_str()),
                    QString::from(outcome.detail.as_str()),
                );
            });
        });
    }
}

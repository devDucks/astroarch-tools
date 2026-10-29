mod backend;
mod manager;
mod translator;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};
use cxx_qt_lib_extras::QApplication;

const BOOTSTRAP_QML: &str = "qrc:/qt/qml/AstroArchSoftwareInstaller/qml/bootstrap.qml";
const MAIN_QML: &str = "qrc:/qt/qml/AstroArchSoftwareInstaller/qml/main.qml";

fn main() {
    // `--version` / `-V` : affiche la version du workspace (Cargo.toml) puis quitte
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }

    let mut app = QApplication::new();
    if let Some(mut app) = app.as_mut() {
        app.as_mut()
            .set_application_name(&QString::from("astroarch-software-installer"));
        app.as_mut()
            .set_organization_name(&QString::from("AstroArch"));
    }

    // Associates the window with org.astroarch.software-installer.desktop (icon and grouping
    // in the taskbar, including under Wayland)
    QGuiApplication::set_desktop_file_name(&QString::from("org.astroarch.software-installer"));

    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        QQuickStyle::set_style(&QString::from("org.kde.desktop"));
    }

    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        engine.as_mut().load(&QUrl::from(BOOTSTRAP_QML));
        engine.as_mut().load(&QUrl::from(MAIN_QML));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

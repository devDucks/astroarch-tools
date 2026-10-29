// 100% Rust application: the interface is in QML (qml/), the logic is in
// bridge.rs (QObject BackupManager exposed to QML via cxx-qt).
//
// Translations require no code: QQmlApplicationEngine automatically loads
// qml_<language>.qm from the i18n folder next to the main QML file
// (see build.rs and translations/)

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

mod bridge;
mod config;
mod disk_analysis;
mod media;
mod storage;

fn main() {
    // `--version` / `-V` : affiche la version du workspace (Cargo.toml) puis quitte
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }

    let mut app = QGuiApplication::new();

    // Associates the window with org.astroarch.backup.desktop (icon and grouping
    // in the taskbar, including under Wayland)
    QGuiApplication::set_desktop_file_name(&QString::from("org.astroarch.backup"));

    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        // qrc:/qt/qml/<module URI>/<file path declared in build.rs>
        engine.load(&QUrl::from("qrc:/qt/qml/AstroArchBackup/qml/Main.qml"));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

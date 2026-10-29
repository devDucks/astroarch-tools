
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

mod launcher;

fn main() {
    // `--version` / `-V` : affiche la version du workspace (Cargo.toml) puis quitte
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }

    // Fusion style (custom QML buttons are not supported by
    // the native style). Any previously defined QT_QUICK_CONTROLS_STYLE variable is respected
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        std::env::set_var("QT_QUICK_CONTROLS_STYLE", "Fusion");
    }

    let mut app = QGuiApplication::new();

    // Associates the window with org.astroarch.control-center.desktop (icon and grouping
    // in the taskbar, including under Wayland)
    QGuiApplication::set_desktop_file_name(&QString::from("org.astroarch.control-center"));

    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        // qrc:/qt/qml/<module URI>/<file path declared in build.rs>
        engine.load(&QUrl::from("qrc:/qt/qml/AstroArchControlCenter/qml/Main.qml"));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

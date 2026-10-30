
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

mod launcher;
mod setup_state;

fn main() {
    // `--version` / `-V` : affiche la version du workspace (Cargo.toml) puis quitte
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }

    // No forced Qt Quick Controls style: on Plasma the platform theme picks
    // org.kde.desktop (Breeze buttons, focus rings, sizes); QT_QUICK_CONTROLS_STYLE still wins

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

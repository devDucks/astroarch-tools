
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

mod launcher;
mod setup_state;

// The software installer's list and install code, shown in the "More software" page:
// the same source files, not a copy. software_manager.rs is a link to its manager.rs
// (cxx-qt wants the QML types of a module in one folder)
#[path = "../../astroarch-software-installer/src/backend.rs"]
mod backend;
mod software_manager;

// The drives of the Image storage page (storage_manager.rs): the backup tool's list,
// compiled in from crates/astroarch-backup (the same files, not a copy)
#[allow(dead_code)]
#[path = "../../astroarch-backup/src/media.rs"]
mod media;
#[path = "../../astroarch-backup/src/storage.rs"]
mod storage;
// The drives offered: the backup tool's list without another system's disk
mod drives;
mod storage_manager;
// The Services page: astroarch-sys-manager's ServiceManager (a link to its bridge.rs)
mod service_manager;

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

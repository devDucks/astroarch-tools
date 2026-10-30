
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

mod launcher;
mod setup_state;

// The software installer's list and install code, shown in the "More software" page:
// the same source files, not a copy. software_manager.rs is a link to its manager.rs
// (cxx-qt wants the QML types of a module in one folder)
#[path = "../../astroarch-software-installer/src/backend.rs"]
mod backend;
mod software_manager;

// The "Data and settings" backup of the backup tool, in the Data backup page: its catalog,
// plan and drive list are compiled in from crates/astroarch-backup (the same files, not a
// copy); data_backup.rs is the page's QObject. The Image storage page uses its drive list
// too (storage_manager.rs)
#[path = "../../astroarch-backup/src/data_profile.rs"]
mod data_profile;
#[allow(dead_code)]
#[path = "../../astroarch-backup/src/media.rs"]
mod media;
#[path = "../../astroarch-backup/src/storage.rs"]
mod storage;
#[path = "../../astroarch-backup/src/config.rs"]
mod config;
#[path = "../../astroarch-backup/src/disk_analysis.rs"]
mod disk_analysis;
// The drives both pages offer: the backup tool's list without another system's disk
mod drives;
mod data_backup;
// The system profile: the backup tool's BackupManager (a link to its bridge.rs, whose
// QML signal names are camelCase)
#[allow(non_snake_case)]
mod system_backup;
mod storage_manager;
// The Services page: astroarch-sys-manager's ServiceManager (a link to its bridge.rs)
mod service_manager;
// The Restore page: the same catalog and plan as the data backup, read in a backup
mod restore;
// The Language page: astroarch-locale-module's LocaleBridge (a link to its bridge.rs) and
// the modules it uses, compiled in from that crate
#[path = "../../astroarch-locale-module/src/geolocation.rs"]
mod geolocation;
#[path = "../../astroarch-locale-module/src/keyboard.rs"]
mod keyboard;
#[path = "../../astroarch-locale-module/src/locale_gen.rs"]
mod locale_gen;
mod locale_bridge;

/// The launcher this window belongs to (icon and name in the task manager): a page opened
/// alone with --single from a menu entry of its own is that entry, else the Control Center
fn desktop_file_name(args: &[String]) -> &'static str {
    let page = args.iter().position(|a| a == "--page").and_then(|i| args.get(i + 1));
    match (args.iter().any(|a| a == "--single"), page.map(String::as_str)) {
        (true, Some("services")) => "org.astroarch.sys-manager",
        (true, Some("software")) => "org.astroarch.software-installer",
        (true, Some("backup")) => "org.astroarch.backup",
        (true, Some("language")) => "org.astroarch.locale-module",
        _ => "org.astroarch.control-center",
    }
}

fn main() {
    // `--version` / `-V` : affiche la version du workspace (Cargo.toml) puis quitte
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }

    // No forced Qt Quick Controls style: on Plasma the platform theme picks
    // org.kde.desktop (Breeze buttons, focus rings, sizes); QT_QUICK_CONTROLS_STYLE still wins

    let mut app = QGuiApplication::new();

    // Associates the window with its .desktop file (icon, name and grouping in the taskbar,
    // including under Wayland): the Control Center's, or the menu entry of a single page
    QGuiApplication::set_desktop_file_name(&QString::from(desktop_file_name(&std::env::args().collect::<Vec<_>>())));

    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        // qrc:/qt/qml/<module URI>/<file path declared in build.rs>
        engine.load(&QUrl::from("qrc:/qt/qml/AstroArchControlCenter/qml/Main.qml"));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

#[cfg(test)]
mod tests {
    use super::desktop_file_name;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_pages_are_their_own_launcher() {
        assert_eq!(desktop_file_name(&args(&["cc", "--page", "services", "--single"])), "org.astroarch.sys-manager");
        assert_eq!(desktop_file_name(&args(&["cc", "--single", "--page", "language"])), "org.astroarch.locale-module");
        assert_eq!(desktop_file_name(&args(&["cc", "--page", "services"])), "org.astroarch.control-center");
        assert_eq!(desktop_file_name(&args(&["cc", "--page", "kiosk", "--single"])), "org.astroarch.control-center");
        assert_eq!(desktop_file_name(&args(&["cc"])), "org.astroarch.control-center");
    }
}

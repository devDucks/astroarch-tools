use std::path::PathBuf;

fn main() {
    let uri = "AstroArchControlCenter";
    astroarch_build::build_qml_app_with_qrcs(
        uri,
        &["qml/Main.qml", "qml/CardButton.qml", "qml/SoftwareList.qml", "qml/KioskSetup.qml", "qml/StorageView.qml", "qml/ServicesView.qml"],
        // software_manager.rs: PackageManager of the software installer (see src/main.rs)
        // storage_manager.rs: drives and the image storage helper (see src/main.rs)
        // service_manager.rs: link to astroarch-sys-manager's bridge.rs (ServiceManager)
        &["src/launcher.rs", "src/setup_state.rs", "src/software_manager.rs", "src/storage_manager.rs", "src/service_manager.rs"],
        astroarch_build::qml_i18n_qrc(uri)
            .into_iter()
            // Logo and background stars, under qrc:/astroarch-control-center/images/
            .chain([PathBuf::from("resources/resources.qrc")]),
    );
}

use std::path::PathBuf;

fn main() {
    let uri = "AstroArchControlCenter";
    astroarch_build::build_qml_app_with_qrcs(
        uri,
        &["qml/Main.qml", "qml/CardButton.qml", "qml/SoftwareList.qml", "qml/KioskSetup.qml", "qml/DataBackupView.qml", "qml/SelectionList.qml", "qml/TickBox.qml", "qml/StorageView.qml", "qml/ServicesView.qml", "qml/RestoreView.qml", "qml/ShareForm.qml", "qml/CardField.qml", "qml/RowsBox.qml"],
        // data_backup.rs: the Data backup page (logic in crates/astroarch-backup);
        // system_backup.rs: link to the backup tool's bridge.rs (BackupManager)
        // software_manager.rs: PackageManager of the software installer (see src/main.rs)
        // storage_manager.rs: drives and the image storage helper (see src/main.rs)
        // service_manager.rs: link to astroarch-sys-manager's bridge.rs (ServiceManager)
        // restore.rs: the Restore page (catalog of the data backup, restore helper)
        &["src/launcher.rs", "src/setup_state.rs", "src/software_manager.rs", "src/data_backup.rs", "src/system_backup.rs", "src/storage_manager.rs", "src/service_manager.rs", "src/restore.rs"],
        astroarch_build::qml_i18n_qrc(uri)
            .into_iter()
            // Logo and background stars, under qrc:/astroarch-control-center/images/
            .chain([PathBuf::from("resources/resources.qrc")]),
    );
}

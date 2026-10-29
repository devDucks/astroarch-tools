fn main() {
    let uri = "AstroArchControlCenter";
    astroarch_build::build_qml_app(
        uri,
        &["qml/Main.qml", "qml/LauncherButton.qml"],
        &["src/launcher.rs"],
        astroarch_build::qml_i18n_qrc(uri),
    );
}

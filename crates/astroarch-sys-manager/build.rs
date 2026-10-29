fn main() {
    let uri = "AstroArchSysManager";
    astroarch_build::build_qml_app(
        uri,
        &["qml/Main.qml"],
        &["src/bridge.rs"],
        astroarch_build::qml_i18n_qrc(uri),
    );
}

fn main() {
    let uri = "AstroArchLocaleModule";
    astroarch_build::build_qml_app(
        uri,
        &["qml/Main.qml", "qml/LanguagePage.qml", "qml/LocationPage.qml", "qml/KeyboardPage.qml", "qml/WorldMapView.qml"],
        &["src/bridge.rs"],
        astroarch_build::qml_i18n_qrc(uri),
    );
}

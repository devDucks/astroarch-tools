// Translations: i18n/*.ts -> .qm -> Qt resources " :/i18n/ " (loaded by src/translator.rs)
fn main() {
    astroarch_build::build_qml_app(
        "AstroArchSoftwareInstaller",
        &["qml/main.qml", "qml/bootstrap.qml"],
        &["src/manager.rs", "src/translator.rs"],
        astroarch_build::translations_qrc("i18n", "/i18n"),
    );
}

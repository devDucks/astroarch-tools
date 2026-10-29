import QtQml
import AstroArchSoftwareInstaller

// Loaded before main.qml: Installs the system translation
QtObject {
    id: root

    property TranslatorManager translator: TranslatorManager {}

    Component.onCompleted: translator.install()
}

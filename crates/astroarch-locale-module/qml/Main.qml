import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import AstroArchLocaleModule

ApplicationWindow {
    id: window
    width: 800
    height: 600
    visible: true
    title: qsTr("Welcome to AstroArch")

    // Wayland: puts back the previous session layout if the user quits without applying
    onClosing: bridge.restore_keyboard_if_unsaved()

    LocaleBridge {
        id: bridge
    }

    // Languages for which translations/qml_<language>.ts exists. For
    // any other language, we fall back to “en” (qml_en = source text):
    // otherwise, Qt would retain the previous translation (e.g., French)
    // when a language without a .qm file is selected
    readonly property var translatedLanguages: ["fr", "it", "es", "de"]

    function uiLanguageFor(code) {
        var lang = code.split(".")[0].split("_")[0]
        return translatedLanguages.indexOf(lang) >= 0 ? lang : "en"
    }

    Connections {
        target: bridge

        // Triggers automatically when poll_geolocation() sets selected_language_code
        function onSelected_language_codeChanged() {
            var langCode = bridge.selected_language_code;
            if (langCode !== "") {
                // Qt.uiLanguage: QQmlApplicationEngine then automatically loads
                // i18n/qml_<language>.qm and re-evaluates the qsTr() functions (see build.rs)
                Qt.uiLanguage = window.uiLanguageFor(langCode);
            }
        }
    }


    Timer {
        id: geoTimer
        interval: 200
        running: !bridge.geo_detected
        repeat: true
        onTriggered: {
            bridge.poll_geolocation()
        }
    }

    Component.onCompleted: {
        bridge.initialize()
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 24
        spacing: 16

        // Step Headers
        RowLayout {
            spacing: 20
            Label {
                text: "1. " + qsTr("Location")
                font.bold: stackLayout.currentIndex === 0
                opacity: stackLayout.currentIndex === 0 ? 1.0 : 0.5
            }
            Label {
                text: "2. " + qsTr("Language")
                font.bold: stackLayout.currentIndex === 1
                opacity: stackLayout.currentIndex === 1 ? 1.0 : 0.5
            }
            Label {
                text: "3. " + qsTr("Keyboard")
                font.bold: stackLayout.currentIndex === 2
                opacity: stackLayout.currentIndex === 2 ? 1.0 : 0.5
            }
        }

        StackLayout {
            id: stackLayout
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: 0

            LocationPage {
                bridge: bridge
            }

            LanguagePage {
                bridge: bridge
            }

            KeyboardPage {
                bridge: bridge
            }
        }

        RowLayout {
            Layout.fillWidth: true

            Button {
                text: qsTr("Back")
                enabled: stackLayout.currentIndex > 0
                onClicked: stackLayout.currentIndex--
            }

            Item { Layout.fillWidth: true }

            Button {
                text: stackLayout.currentIndex === 2 ? qsTr("Finish") : qsTr("Next")
                highlighted: true
                onClicked: {
                    if (stackLayout.currentIndex < 2) {
                        stackLayout.currentIndex++
                    } else {
                        if (bridge.persist_configuration()) {
                            doneDialog.open()
                        } else {
                            errorDialog.open()
                        }
                    }
                }
            }
        }
    }

    Dialog {
        id: doneDialog
        anchors.centerIn: parent
        modal: true
        // Fixed width: A Label that wraps to the next line in a Dialog with
        // default width causes a binding loop (implicitWidth)
        width: 440
        title: qsTr("Settings applied")
        standardButtons: Dialog.Ok
        onAccepted: Qt.quit()
        Label {
            text: qsTr("Log out and back in for the new language to apply everywhere.")
            wrapMode: Text.WordWrap
        }
    }

    Dialog {
        id: errorDialog
        anchors.centerIn: parent
        modal: true
        // Fixed width: A Label that wraps to the next line in a Dialog with
        // default width causes a binding loop (implicitWidth)
        width: 440
        title: qsTr("Could not apply settings")
        standardButtons: Dialog.Ok
        Label {
            text: qsTr("Authorization was refused or the system commands failed.")
            wrapMode: Text.WordWrap
        }
    }
}

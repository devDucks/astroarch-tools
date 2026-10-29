import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    required property var bridge
    spacing: 16

    Label {
        text: qsTr("Choose your language")
        font.pixelSize: 20
        font.bold: true
    }

    ComboBox {
        id: languageCombo
        Layout.fillWidth: true
        Layout.maximumWidth: 480
        model: bridge.language_display_names

        Component.onCompleted: updateSelection()

        Connections {
            target: bridge
            function onSelected_language_codeChanged() {
                languageCombo.updateSelection()
            }
        }

        function updateSelection() {
            var idx = bridge.language_codes.indexOf(bridge.selected_language_code)
            if (idx >= 0) {
                currentIndex = idx
            }
        }

        onActivated: (index) => {
            if (index >= 0 && index < bridge.language_codes.length) {
                var code = bridge.language_codes[index]
                // set_language changes selected_language_code; Main.qml
                // responds to this change and switches the interface language.
                bridge.set_language(code)
            }
        }
    }

    Label {
        visible: !bridge.geo_detected
        text: "..."
        opacity: 0.6
    }

    Label {
        visible: bridge.geo_detected && !bridge.has_internet
        text: qsTr("No internet connection detected — defaulting to English.")
        color: "#b58900"
        wrapMode: Text.WordWrap
        Layout.fillWidth: true
    }

    Item { Layout.fillHeight: true }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    required property var bridge
    spacing: 16

    signal languageChosen()
    // The location is being guessed (full flow, not "--page")
    property bool guessing: true

    // Names from Qt's locale data, each in its own language: "Italiano (Italia)"
    function nameOf(code) {
        // "sr_RS@latin": Qt writes the Latin script as sr_Latn_RS; other variants
        // ("ca_ES@valencia") are named after the base locale, with the variant
        const [base, variant] = code.split("@")
        const [language, country] = base.split("_")
        const qtName = variant === "latin" && country ? language + "_Latn_" + country : base
        const l = Qt.locale(qtName)
        if (l.name === "C")
            return code
        const lang = l.nativeLanguageName.charAt(0).toUpperCase() + l.nativeLanguageName.slice(1)
        // Qt may not know this pair and give another country of the language
        const where = country ? " (" + (l.name === qtName ? l.nativeTerritoryName : country) + ")" : ""
        return lang + where + (variant && variant !== "latin" ? " · " + variant : "")
    }
    readonly property var languages: Array.from(bridge.language_codes)
        .map(code => ({ code: code, name: nameOf(code) }))
        .sort((a, b) => a.name.localeCompare(b.name))

    Label {
        text: qsTr("Choose your language")
        font.pixelSize: 20
        font.bold: true
    }

    ComboBox {
        id: languageCombo
        Layout.fillWidth: true
        Layout.maximumWidth: 480
        model: root.languages.map(l => l.name)

        Component.onCompleted: updateSelection()

        Connections {
            target: bridge
            function onSelected_language_codeChanged() {
                languageCombo.updateSelection()
            }
        }

        function updateSelection() {
            var idx = root.languages.findIndex(l => l.code === bridge.selected_language_code)
            if (idx >= 0) {
                currentIndex = idx
            }
        }

        onActivated: (index) => {
            if (index >= 0 && index < root.languages.length) {
                bridge.set_language(root.languages[index].code)
                // Main.qml switches the interface to it
                root.languageChosen()
            }
        }
    }

    Label {
        visible: root.guessing && !bridge.geo_detected
        text: "..."
        opacity: 0.6
    }

    Label {
        visible: root.guessing && bridge.geo_detected && !bridge.has_internet
        text: qsTr("No internet connection detected — defaulting to English.")
        color: "#b58900"
        wrapMode: Text.WordWrap
        Layout.fillWidth: true
    }

    Item { Layout.fillHeight: true }
}

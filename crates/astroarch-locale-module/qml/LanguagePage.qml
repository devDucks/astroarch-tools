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
    // The whole list of languages, not the country's
    property bool otherLanguage: false

    // Languages of the chosen country (restcountries.com, else the installed locales)
    readonly property var countryLanguages: Array.from(bridge.country_language_codes)
        .map(code => ({ code: code, name: nameOf(code) }))
    readonly property bool showCountryLanguages: bridge.selected_country_code !== "" && !otherLanguage
        && (countryLanguages.length > 0 || bridge.languages_source === "loading")
    readonly property string countryName: {
        const i = Array.from(bridge.country_codes).indexOf(bridge.selected_country_code)
        if (i < 0)
            return bridge.selected_country_code
        for (const loc of bridge.country_locales[i].split(",")) {
            const l = Qt.locale(loc)
            if (l.name === loc && l.nativeTerritoryName)
                return l.nativeTerritoryName
        }
        return bridge.selected_country_code
    }
    // One of the country's languages is chosen (or the whole list is shown)
    readonly property bool picked: bridge.languages_source !== "loading" && (!showCountryLanguages
        || countryLanguages.some(l => l.code === bridge.selected_language_code))

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

    // The country's languages: the user chooses one of them
    ColumnLayout {
        Layout.fillWidth: true
        visible: root.showCountryLanguages
        spacing: 6

        Label {
            visible: bridge.languages_source === "loading"
            text: qsTr("Looking up the languages of %1…").arg(root.countryName)
            opacity: 0.6
        }
        Label {
            visible: root.countryLanguages.length > 0
            text: qsTr("Languages of %1: choose yours.").arg(root.countryName)
            font.bold: true
        }
        Label {
            visible: bridge.languages_source === "offline" && root.countryLanguages.length > 0
            text: qsTr("No connection: these are the languages installed for %1.").arg(root.countryName)
            color: "#b58900"
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }
        Repeater {
            model: root.countryLanguages
            RadioButton {
                required property var modelData
                text: modelData.name
                checked: modelData.code === bridge.selected_language_code
                onClicked: {
                    bridge.set_language(modelData.code)
                    // Main.qml switches the interface to it
                    root.languageChosen()
                }
            }
        }
        Button {
            visible: bridge.languages_source !== "loading"
            text: qsTr("Another language…")
            onClicked: root.otherLanguage = true
        }
    }

    // Any language: no country chosen, or a language that is not one of the country's
    ColumnLayout {
        Layout.fillWidth: true
        visible: !root.showCountryLanguages
        spacing: 6

        Button {
            visible: root.otherLanguage && root.countryLanguages.length > 0
            text: qsTr("Back to the languages of %1").arg(root.countryName)
            onClicked: root.otherLanguage = false
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
    }

    Label {
        visible: root.guessing && !bridge.geo_detected
        text: "..."
        opacity: 0.6
    }

    Label {
        visible: root.guessing && bridge.geo_detected && !bridge.has_internet
        text: qsTr("No internet connection detected: the country and its languages cannot be looked up.")
        color: "#b58900"
        wrapMode: Text.WordWrap
        Layout.fillWidth: true
    }

    Item { Layout.fillHeight: true }
}

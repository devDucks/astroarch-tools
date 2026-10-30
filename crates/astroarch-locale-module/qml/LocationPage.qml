import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    required property var bridge
    spacing: 16

    // A country chosen by the user also chooses its language
    signal languageChosen()
    // The location is being guessed (full flow, not "--page")
    property bool guessing: true

    // Country names from Qt's locale data, in the country's own language ("Italia"),
    // taken from the first of its locales that Qt knows
    function nameOf(code, locales) {
        for (const loc of locales.split(",")) {
            const l = Qt.locale(loc)
            if (l.name === loc && l.nativeTerritoryName)
                return l.nativeTerritoryName
        }
        return code
    }
    readonly property var countries: Array.from(bridge.country_codes)
        .map((code, i) => ({ code: code, name: nameOf(code, bridge.country_locales[i]) }))
        .sort((a, b) => a.name.localeCompare(b.name))

    Label {
        text: qsTr("Choose your location")
        font.pixelSize: 20
        font.bold: true
    }

    ComboBox {
        id: countryCombo
        Layout.fillWidth: true
        Layout.maximumWidth: 480
        model: root.countries.map(c => c.name)

        Component.onCompleted: updateSelection()

        Connections {
            target: bridge
            function onSelected_country_codeChanged() {
                countryCombo.updateSelection()
            }
        }

        function updateSelection() {
            var idx = root.countries.findIndex(c => c.code === bridge.selected_country_code)
            if (idx >= 0) currentIndex = idx
        }

        onActivated: (index) => {
            if (index >= 0 && index < root.countries.length) {
                bridge.set_country(root.countries[index].code)
                root.languageChosen()
            }
        }
    }

    WorldMapView {
        id: worldMap
        Layout.fillWidth: true
        Layout.fillHeight: true
        selectedCountryCode: bridge.selected_country_code

        // Listening to the signal transmitted by WorldMapView
        Connections {
            target: worldMap
            function onCountryPicked(code) {
                bridge.set_country(code)
                root.languageChosen()
            }
        }
    }

    Label {
        visible: root.guessing && !bridge.geo_detected
        text: qsTr("Detecting location...")
        opacity: 0.6
    }

    Item { Layout.fillHeight: true }
}

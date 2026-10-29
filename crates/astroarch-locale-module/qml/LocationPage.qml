import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    required property var bridge
    spacing: 16

    Label {
        text: qsTr("Choose your location")
        font.pixelSize: 20
        font.bold: true
    }

    ComboBox {
        id: countryCombo
        Layout.fillWidth: true
        Layout.maximumWidth: 480
        model: bridge.country_display_names

        Component.onCompleted: updateSelection()

        Connections {
            target: bridge
            function onSelected_country_codeChanged() {
                countryCombo.updateSelection()
            }
        }

        function updateSelection() {
            var idx = bridge.country_codes.indexOf(bridge.selected_country_code)
            if (idx >= 0) currentIndex = idx
        }

        onActivated: (index) => {
            if (index >= 0 && index < bridge.country_codes.length) {
                var code = bridge.country_codes[index]
                bridge.set_country(code)
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
            }
        }
    }

    Label {
        visible: !bridge.geo_detected
        text: qsTr("Detecting location...")
        opacity: 0.6
    }

    Item { Layout.fillHeight: true }
}

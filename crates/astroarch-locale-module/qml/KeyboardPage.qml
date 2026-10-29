import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    required property var bridge
    spacing: 16

    Label {
        text: qsTr("Choose your keyboard layout")
        font.pixelSize: 20
        font.bold: true
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 12

        ColumnLayout {
            Layout.fillWidth: true
            Label { text: qsTr("Layout") }

            ComboBox {
                id: layoutCombo
                Layout.fillWidth: true
                model: bridge.keyboard_layout_names

                Component.onCompleted: syncLayout()

                Connections {
                    target: bridge
                    function onSelected_layout_codeChanged() {
                        layoutCombo.syncLayout()
                    }
                }

                function syncLayout() {
                    var idx = bridge.keyboard_layout_codes.indexOf(bridge.selected_layout_code)
                    if (idx >= 0) currentIndex = idx
                }

                onActivated: (index) => {
                    if (index >= 0 && index < bridge.keyboard_layout_codes.length) {
                        var code = bridge.keyboard_layout_codes[index]
                        bridge.set_keyboard_layout(code)
                        bridge.apply_keyboard_for_testing()
                    }
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Label { text: qsTr("Variant") }

            ComboBox {
                id: variantCombo
                Layout.fillWidth: true

                // First entry = no variant (the layout's default one)
                readonly property var variantCodes: [""].concat(bridge.keyboard_variant_codes)
                model: [qsTr("Default")].concat(bridge.keyboard_variant_names)

                Component.onCompleted: syncVariant()

                Connections {
                    target: bridge
                    function onSelected_variant_codeChanged() {
                        variantCombo.syncVariant()
                    }
                    function onKeyboard_variant_namesChanged() {
                        variantCombo.syncVariant()
                    }
                }

                function syncVariant() {
                    var idx = variantCodes.indexOf(bridge.selected_variant_code)
                    currentIndex = idx >= 0 ? idx : 0
                }

                onActivated: (index) => {
                    if (index >= 0 && index < variantCodes.length) {
                        var code = variantCodes[index]
                        bridge.set_keyboard_variant(code)
                        bridge.apply_keyboard_for_testing()
                    }
                }
            }
        }
    }

    Label {
        text: qsTr("Type here to test your keyboard layout")
    }

    ScrollView {
        Layout.fillWidth: true
        Layout.preferredHeight: 120

        TextArea {
            placeholderText: qsTr("Type something...")
            wrapMode: TextEdit.Wrap
        }
    }

    Item { Layout.fillHeight: true }
}

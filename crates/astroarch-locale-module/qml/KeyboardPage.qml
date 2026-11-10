import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    required property var bridge
    spacing: 16

    // The whole list of layouts, not the proposed ones
    property bool otherKeyboard: false

    // Keyboards proposed for the country and the language
    readonly property var proposals: Array.from(bridge.keyboard_suggestion_layouts)
        .map((layout, i) => ({ layout: layout, variant: bridge.keyboard_suggestion_variants[i] || "",
                               name: bridge.keyboard_suggestion_names[i] || layout }))
    readonly property bool proposalSelected: proposals.some(p =>
        p.layout === bridge.selected_layout_code && p.variant === bridge.selected_variant_code)
    // The whole list: asked for, or the keyboard is none of the proposed ones
    readonly property bool listShown: otherKeyboard || proposals.length === 0 || !proposalSelected

    Label {
        text: qsTr("Choose your keyboard layout")
        font.pixelSize: 20
        font.bold: true
    }

    // Proposed for the country and the language; another one for a keyboard bought abroad
    ColumnLayout {
        Layout.fillWidth: true
        visible: root.proposals.length > 0
        spacing: 6

        Label {
            text: qsTr("Proposed for your country and language.")
            font.bold: true
        }
        Repeater {
            model: root.proposals
            RadioButton {
                required property var modelData
                text: modelData.name
                checked: modelData.layout === bridge.selected_layout_code
                         && modelData.variant === bridge.selected_variant_code
                onClicked: {
                    bridge.choose_keyboard(modelData.layout, modelData.variant)
                    bridge.apply_keyboard_for_testing()
                    root.otherKeyboard = false
                }
            }
        }
        Button {
            visible: !root.listShown
            text: qsTr("Another keyboard…")
            onClicked: root.otherKeyboard = true
        }
        Button {
            visible: root.listShown && root.otherKeyboard && root.proposalSelected
            text: qsTr("Back to the proposed keyboards")
            onClicked: root.otherKeyboard = false
        }
    }

    RowLayout {
        Layout.fillWidth: true
        visible: root.listShown
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

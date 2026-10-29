import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Button {
    id: control

    property string glyph: ""
    property string heading: ""
    property string subtitle: ""
    property color accent: "#4f8cff"

    implicitHeight: 110
    padding: 16
    hoverEnabled: true

    background: Rectangle {
        radius: 12
        color: control.down ? "#1f2a3d" : (control.hovered ? "#19233a" : "#141c2b")
        border.width: control.visualFocus ? 2 : 1
        border.color: (control.hovered || control.visualFocus) ? control.accent : "#263149"
    }

    contentItem: RowLayout {
        spacing: 16

        Rectangle {
            Layout.preferredWidth: 56
            Layout.preferredHeight: 56
            radius: 28
            color: Qt.rgba(control.accent.r, control.accent.g, control.accent.b, 0.18)

            Label {
                anchors.centerIn: parent
                text: control.glyph
                color: control.accent
                font.pixelSize: 28
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4

            Label {
                Layout.fillWidth: true
                text: control.heading
                color: "#e6edf3"
                font.pixelSize: 18
                font.bold: true
                elide: Text.ElideRight
            }

            Label {
                Layout.fillWidth: true
                text: control.subtitle
                color: "#8b98a9"
                font.pixelSize: 13
                wrapMode: Text.WordWrap
            }
        }
    }
}

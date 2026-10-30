import QtQuick
import QtQuick.Layouts

// White box with rows, as the Control Center's own rows (Data backup, Restore)
Rectangle {
    default property alias content: box.data
    Layout.fillWidth: true
    implicitHeight: box.implicitHeight
    radius: 8
    color: "#ffffff"
    border.color: "#d3dbe2"
    clip: true
    ColumnLayout {
        id: box
        width: parent.width
        spacing: 0
    }
}

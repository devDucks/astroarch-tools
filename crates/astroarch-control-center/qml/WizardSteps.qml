import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// "Step 2 of 3", the name of the step and the segmented bar: the header of the Control
// Center's wizards (Data backup, Restore, Language)
ColumnLayout {
    id: header

    required property var steps         // names of the steps
    required property int step          // 1, 2...
    property bool showNext: false       // "Next: <name of the next step>"

    Layout.fillWidth: true
    spacing: 12

    RowLayout {
        Layout.fillWidth: true
        spacing: 10
        Label {
            text: qsTr("Step %1 of %2").arg(header.step).arg(header.steps.length)
            color: "#1d6fa5"
            font.bold: true
        }
        Label {
            Layout.fillWidth: true
            text: header.steps[header.step - 1] || ""
            color: "#1f2d3a"
            font.bold: true
            elide: Text.ElideRight
        }
        Label {
            visible: header.showNext && header.step < header.steps.length
            text: qsTr("Next: %1").arg(header.step < header.steps.length ? header.steps[header.step] : "")
            color: "#5d6d7e"
            font: Kirigami.Theme.smallFont
        }
    }
    RowLayout {
        Layout.fillWidth: true
        spacing: 4
        Accessible.ignored: true
        Repeater {
            model: header.steps.length
            Rectangle {
                required property int index
                Layout.fillWidth: true
                implicitHeight: 6
                radius: 3
                color: index < header.step ? "#1d6fa5" : "#c9d3db"
            }
        }
    }
}

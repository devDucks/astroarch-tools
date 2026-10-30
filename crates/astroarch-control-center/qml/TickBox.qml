import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

// Square tick box of the light card, three states (Qt.Checked, PartiallyChecked,
// Unchecked); forced: greyed, it cannot be unticked
AbstractButton {
    id: tick
    property int tickState: Qt.Unchecked
    property bool forced: false
    implicitWidth: 28
    implicitHeight: 28
    focusPolicy: Qt.StrongFocus
    Accessible.role: Accessible.CheckBox
    Accessible.checkable: true
    Accessible.checked: tickState === Qt.Checked
    Accessible.onToggleAction: clicked()
    Accessible.onPressAction: clicked()
    background: null
    contentItem: Item {
        Rectangle {
            anchors.centerIn: parent
            width: 20
            height: 20
            radius: 4
            color: tick.tickState === Qt.Unchecked ? "#ffffff" : (tick.forced ? "#8fa6ba" : "#1d6fa5")
            border.width: 2
            border.color: tick.tickState === Qt.Unchecked ? "#7d8994" : color
            Kirigami.Icon {
                anchors.centerIn: parent
                width: 14
                height: 14
                source: "checkmark"
                isMask: true
                color: "#ffffff"
                visible: tick.tickState === Qt.Checked
            }
            Rectangle {
                anchors.centerIn: parent
                width: 10
                height: 2
                color: "#ffffff"
                visible: tick.tickState === Qt.PartiallyChecked
            }
        }
        Rectangle {
            anchors.fill: parent
            radius: 6
            color: "transparent"
            border.width: 2
            border.color: "#1d6fa5"
            visible: tick.visualFocus
        }
    }
}

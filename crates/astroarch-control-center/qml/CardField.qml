import QtQuick
import QtQuick.Controls

// Text field of the light card, drawn here like CardButton
TextField {
    id: field
    color: "#232629"
    placeholderTextColor: "#7a828a"
    background: Rectangle {
        radius: 4
        color: "#ffffff"
        border.width: field.activeFocus ? 2 : 1
        border.color: field.activeFocus ? "#3daee9" : "#9aa4ad"
    }
}

import QtQuick
import QtQuick.Controls

// Button of the light card, drawn here: the Plasma style would draw it with the
// dark theme. The primary one (highlighted) is filled with the selection colour; the
// chosen one of a choice (checked) is only tinted, as it is not an action. destructive:
// an action that cannot be undone, in Breeze's negative red (filled when highlighted,
// else red text and border)
Button {
    id: cardButton
    property bool destructive: false

    leftPadding: 16
    rightPadding: 16
    topPadding: 9
    bottomPadding: 9
    implicitHeight: Math.max(40, implicitContentHeight + topPadding + bottomPadding)

    contentItem: Label {
        text: cardButton.text
        font: cardButton.font
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
        color: !cardButton.enabled ? "#7a828a"
             : cardButton.highlighted ? "#ffffff"
             : (cardButton.destructive ? "#b8333f" : "#232629")
    }

    background: Rectangle {
        radius: 4
        color: !cardButton.enabled ? (cardButton.highlighted ? (cardButton.destructive ? "#e3a5ab" : "#9db7cb") : "#eef0f2")
             : cardButton.highlighted && cardButton.destructive ? (cardButton.down ? "#b8333f" : (cardButton.hovered ? "#c93c4b" : "#da4453"))
             : cardButton.highlighted ? (cardButton.down ? "#185d8a" : (cardButton.hovered ? "#1a6596" : "#1d6fa5"))
             : cardButton.checked ? "#e3f0f8"
             : (cardButton.down ? "#dde7ef" : (cardButton.hovered ? "#eef3f7" : "#fcfcfc"))
        border.width: cardButton.visualFocus || cardButton.checked ? 2 : 1
        border.color: cardButton.visualFocus ? "#3daee9"
                    : (!cardButton.enabled ? "#c9ced3"
                       : cardButton.destructive ? "#da4453"
                       : (cardButton.highlighted || cardButton.checked ? "#1d6fa5" : "#9aa4ad"))
    }
}

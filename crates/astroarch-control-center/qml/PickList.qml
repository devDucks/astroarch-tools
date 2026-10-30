import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// A long list of the light card with a search field: one entry is the chosen one
// (countries, languages, keyboard layouts of the Language view)
ColumnLayout {
    id: pick

    property var items: []              // [{ code, name, detail }]
    property string current: ""         // code of the chosen entry
    property string placeholder: qsTr("Search…")
    property bool searchable: true
    property int listHeight: 220
    property string accessibleName: ""
    signal picked(string code)

    readonly property var shown: {
        const q = search.text.trim().toLocaleLowerCase()
        if (q === "")
            return pick.items
        return pick.items.filter(i => (i.name + " " + (i.detail || "") + " " + i.code).toLocaleLowerCase().includes(q))
    }

    spacing: 6

    // A new start of the wizard: no search left from before
    function clear() {
        search.text = ""
    }

    function scrollToCurrent() {
        const i = pick.shown.findIndex(e => e.code === pick.current)
        if (i >= 0)
            list.positionViewAtIndex(i, ListView.Center)
    }
    onCurrentChanged: Qt.callLater(scrollToCurrent)
    onShownChanged: Qt.callLater(scrollToCurrent)
    onVisibleChanged: if (visible) Qt.callLater(scrollToCurrent)

    CardField {
        id: search
        visible: pick.searchable
        Layout.fillWidth: true
        placeholderText: pick.placeholder
        Accessible.name: pick.accessibleName + " · " + pick.placeholder
        // Enter with one match left chooses it
        onAccepted: if (pick.shown.length === 1) pick.picked(pick.shown[0].code)
    }

    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: pick.listHeight
        radius: 8
        color: "#ffffff"
        border.color: "#d3dbe2"
        clip: true

        ListView {
            id: list
            anchors.fill: parent
            anchors.margins: 1
            clip: true
            model: pick.shown
            boundsBehavior: Flickable.StopAtBounds
            Accessible.role: Accessible.List
            Accessible.name: pick.accessibleName
            ScrollBar.vertical: ScrollBar {
                policy: list.contentHeight > list.height ? ScrollBar.AlwaysOn : ScrollBar.AsNeeded
            }

            delegate: ItemDelegate {
                id: entry

                required property var modelData
                required property int index
                readonly property bool chosen: modelData.code === pick.current

                width: ListView.view.width - (list.contentHeight > list.height ? 12 : 0)
                text: modelData.name
                Accessible.role: Accessible.ListItem
                Accessible.name: modelData.name + (modelData.detail ? ", " + modelData.detail : "")
                Accessible.selected: chosen
                onClicked: pick.picked(modelData.code)

                background: Rectangle {
                    color: entry.chosen ? "#e3f0f8" : (entry.down ? "#dde7ef" : (entry.hovered ? "#eef3f7" : "transparent"))
                    border.width: entry.visualFocus ? 2 : 0
                    border.color: "#3daee9"
                }
                contentItem: RowLayout {
                    spacing: 10
                    Label {
                        Layout.fillWidth: true
                        text: entry.modelData.name
                        color: "#1f2d3a"
                        font.bold: entry.chosen
                        elide: Text.ElideRight
                    }
                    Label {
                        visible: !!entry.modelData.detail
                        Layout.maximumWidth: entry.width * 0.45
                        text: entry.modelData.detail || ""
                        color: "#5d6d7e"
                        font: Kirigami.Theme.smallFont
                        elide: Text.ElideRight
                    }
                    Kirigami.Icon {
                        source: "checkmark"
                        color: "#1d6fa5"
                        isMask: true
                        opacity: entry.chosen ? 1 : 0
                        Layout.preferredWidth: 16
                        Layout.preferredHeight: 16
                    }
                }
            }
        }

        Label {
            anchors.centerIn: parent
            visible: pick.shown.length === 0
            text: qsTr("Nothing found")
            color: "#5d6d7e"
        }
    }
}

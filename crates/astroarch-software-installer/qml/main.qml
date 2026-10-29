import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import AstroArchSoftwareInstaller

ApplicationWindow {
    id: win

    width: 780
    height: 640
    minimumWidth: 480
    minimumHeight: 360
    visible: true
    title: qsTr("Software installer")

    property string statusMessage: ""
    property bool loaded: false
    property string search: ""
    property string categoryFilter: ""

    ListModel {
        id: softwareModel
    }

    PackageManager {
        id: manager

        onPackagesLoaded: function (json) {
            var items = []
            try {
                items = JSON.parse(json)
            } catch (e) {
                items = []
            }
            softwareModel.clear()
            var cats = {}
            for (var i = 0; i < items.length; ++i) {
                var it = items[i]
                var cat = (it.category && it.category.length > 0) ? it.category : qsTr("Other")
                cats[cat] = true
                softwareModel.append({
                    "pkg": it.name,
                    "title": it.title,
                    "description": it.description,
                    "category": cat,
                    "installed": it.installed,
                    "available": it.available,
                    "aur": it.aur,
                    "working": false,
                    "rev": 0
                })
            }
            categoryBox.model = [qsTr("All categories")].concat(Object.keys(cats).sort())
            categoryBox.currentIndex = 0
            win.categoryFilter = ""
            win.statusMessage = ""
            win.loaded = true
        }

        onLoadFailed: function (message) {
            softwareModel.clear()
            win.statusMessage = qsTr("Cannot read the software list: %1").arg(message)
            win.loaded = true
        }

        onOperationFinished: function (name, ok, installed, errorKind, detail) {
            var title = name
            for (var i = 0; i < softwareModel.count; ++i) {
                var row = softwareModel.get(i)
                if (row.pkg === name) {
                    title = row.title
                    softwareModel.setProperty(i, "installed", installed)
                    softwareModel.setProperty(i, "working", false)
                    // forces the switch to revert to its actual state (even if the operation fails)
                    softwareModel.setProperty(i, "rev", row.rev + 1)
                    break
                }
            }
            if (ok) {
                win.statusMessage = installed ? qsTr("%1 installed.").arg(title)
                                              : qsTr("%1 removed.").arg(title)
            } else if (errorKind === "auth") {
                win.statusMessage = qsTr("Authentication cancelled or refused.")
            } else {
                win.statusMessage = qsTr("Operation failed: %1").arg(detail)
            }
        }
    }

    Component.onCompleted: manager.reload()

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            spacing: 8

            TextField {
                id: searchField
                Layout.fillWidth: true
                placeholderText: qsTr("Search software…")
                onTextChanged: win.search = text.toLowerCase()
            }

            ComboBox {
                id: categoryBox
                model: [qsTr("All categories")]
                onActivated: win.categoryFilter = (currentIndex === 0 ? "" : currentText)
            }

            ToolButton {
                text: qsTr("Reload")
                enabled: !manager.busy
                onClicked: manager.reload()
            }
        }
    }

    footer: ToolBar {
        visible: win.statusMessage.length > 0
        height: visible ? implicitHeight : 0
        Label {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
            text: win.statusMessage
        }
    }

    ListView {
        id: view
        anchors.fill: parent
        clip: true
        model: softwareModel
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}

        delegate: Item {
            id: row

            required property int index
            required property string pkg
            required property string title
            required property string description
            required property string category
            required property bool installed
            required property bool available
            required property bool aur
            required property bool working
            required property int rev

            readonly property bool matches: {
                if (win.categoryFilter !== "" && category !== win.categoryFilter)
                    return false
                if (win.search === "")
                    return true
                return title.toLowerCase().indexOf(win.search) >= 0
                    || pkg.toLowerCase().indexOf(win.search) >= 0
                    || description.toLowerCase().indexOf(win.search) >= 0
            }

            visible: matches
            width: ListView.view ? ListView.view.width : 0
            height: matches ? content.implicitHeight + 20 : 0

            function requestChange(wantInstalled) {
                softwareModel.setProperty(row.index, "working", true)
                if (wantInstalled && row.aur)
                    win.statusMessage = qsTr("Building and installing %1 from the AUR… this can take a long time.").arg(row.title)
                else
                    win.statusMessage = (wantInstalled ? qsTr("Installing %1…")
                                                       : qsTr("Removing %1…")).arg(row.title)
                manager.apply(row.pkg, wantInstalled, row.aur)
            }

            RowLayout {
                id: content
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                spacing: 12

                // Switch (or activity indicator during operation)
                Item {
                    Layout.preferredWidth: 64
                    Layout.preferredHeight: sw.implicitHeight
                    Layout.alignment: Qt.AlignTop

                    Switch {
                        id: sw
                        anchors.centerIn: parent
                        visible: !row.working
                        checked: row.installed
                        enabled: !manager.busy && (row.installed || row.available)
                        ToolTip.visible: hovered
                        ToolTip.delay: 600
                        ToolTip.text: checked ? qsTr("Click to remove") : qsTr("Click to install")
                        onToggled: row.requestChange(checked)
                    }

                    BusyIndicator {
                        anchors.centerIn: parent
                        width: 32
                        height: 32
                        running: row.working
                        visible: row.working
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        Label {
                            text: row.title
                            font.bold: true
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        Label {
                            visible: row.aur
                            text: "AUR"
                            font.pixelSize: 10
                            font.bold: true
                            leftPadding: 6
                            rightPadding: 6
                            topPadding: 1
                            bottomPadding: 1
                            color: row.palette.highlightedText
                            background: Rectangle {
                                radius: 3
                                color: row.palette.highlight
                            }
                        }
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: text.length > 0
                        text: row.description
                        wrapMode: Text.Wrap
                        opacity: 0.75
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: !row.installed && !row.available
                        text: row.aur ? qsTr("Requires paru (AUR helper)")
                                      : qsTr("Not available in the repositories")
                        font.italic: true
                        opacity: 0.6
                    }
                }
            }

            onRevChanged: sw.checked = row.installed

            Rectangle {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                height: 1
                color: row.palette.mid
                opacity: 0.4
            }
        }
    }

    // Loading / Empty list
    ColumnLayout {
        anchors.centerIn: parent
        spacing: 12
        visible: !win.loaded || (softwareModel.count === 0 && win.statusMessage.length === 0)

        BusyIndicator {
            Layout.alignment: Qt.AlignHCenter
            running: !win.loaded
            visible: !win.loaded
        }
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: win.loaded ? qsTr("No software found.") : qsTr("Loading the list…")
            opacity: 0.7
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// External window: tree view of folders that can be excluded from the backup
// (your personal folder and, if it exists, the shared data folder), with a
// checkbox for each folder (checked = excluded, including subfolders)
Window {
    id: win
    width: 700
    height: 680
    minimumWidth: 480
    minimumHeight: 360
    title: qsTr("Select the folders to exclude")
    color: "#ecf0f1"
    flags: Qt.Dialog

    // BackupManager (provided by Main.qml)
    property var backend: null
    // absolute path -> true (folders checked)
    property var selectedSet: ({})
    // absolute path -> size in bytes
    property var sizeCache: ({})
    property bool showHidden: false
    property string homePath: ""
    property string sharedPath: ""

    signal selectionAccepted(var paths)

    function formatBytes(n) {
        var units = ["B", "KiB", "MiB", "GiB", "TiB"]
        var v = Number(n)
        var i = 0
        while (v >= 1024 && i < units.length - 1) {
            v /= 1024
            i++
        }
        return v.toFixed(i === 0 ? 0 : 1) + " " + units[i]
    }

    function childPath(parentPath, name) {
        return parentPath.replace(/\/+$/, "") + "/" + name
    }

    function sizeLabel(path) {
        return sizeCache[path] !== undefined ? formatBytes(sizeCache[path]) : "…"
    }

    function openWith(paths) {
        selectedSet = {}
        for (var i = 0; i < paths.length; ++i)
            selectedSet[paths[i]] = true
        sizeCache = {}
        homePath = backend.home_path()
        sharedPath = backend.shared_data_path()
        console.log("Folder selection: home =", homePath, "- shared data folder =",
                    sharedPath === "" ? "(not found)" : sharedPath)
        reload()
        show()
        raise()
        requestActivate()
    }

    // Roots: shared data folder (if it exists, first), then
    // personal folder (expanded, cannot be excluded in its entirety)
    function reload() {
        treeModel.clear()

        if (sharedPath !== "") {
            var sharedChildren = JSON.parse(backend.list_subfolders(sharedPath, showHidden))
            treeModel.append({
                "path": sharedPath,
                "name": sharedPath,
                "label": qsTr("Shared data folder") + "  " + sharedPath,
                "depth": 0,
                "hasChildren": sharedChildren.length > 0,
                "excludable": true,
                "locked": false,
                "expanded": false,
                "checked": selectedSet[sharedPath] === true,
                "inherited": false,
                "sizeText": sizeLabel(sharedPath)
            })
            backend.request_path_size(sharedPath)
        }

        var homeIndex = treeModel.count
        treeModel.append({
            "path": homePath,
            "name": homePath,
            "label": qsTr("Your home folder") + "  " + homePath,
            "depth": 0,
            "hasChildren": true,
            "excludable": false,
            "locked": false,
            "expanded": true,
            "checked": false,
            "inherited": false,
            "sizeText": ""
        })
        insertChildren(homeIndex, homePath, 1)
    }

    // Inserts the subfolders of parentPath immediately after the `index` line
    function insertChildren(index, parentPath, depth) {
        var items = JSON.parse(backend.list_subfolders(parentPath, showHidden))
        for (var i = 0; i < items.length; ++i) {
            var path = childPath(parentPath, items[i].name)
            treeModel.insert(index + 1 + i, {
                "path": path,
                "name": items[i].name,
                "label": items[i].name,
                "depth": depth,
                "hasChildren": items[i].hasChildren,
                "excludable": items[i].excludable,
                "locked": items[i].locked === true,
                "expanded": false,
                "checked": selectedSet[path] === true,
                "inherited": false,
                "sizeText": sizeLabel(path)
            })
        }
        refreshInherited()
        backend.request_folder_sizes(parentPath, showHidden)
    }

    function toggleExpand(index) {
        var row = treeModel.get(index)
        if (row.expanded) {
            var n = 0
            while (index + 1 + n < treeModel.count && treeModel.get(index + 1 + n).depth > row.depth)
                n++
            treeModel.remove(index + 1, n)
            treeModel.setProperty(index, "expanded", false)
        } else {
            insertChildren(index, row.path, row.depth + 1)
            treeModel.setProperty(index, "expanded", true)
        }
    }

    function setChecked(index, value) {
        var row = treeModel.get(index)
        if (!row.excludable)
            return
        if (value)
            selectedSet[row.path] = true
        else
            delete selectedSet[row.path]
        treeModel.setProperty(index, "checked", value)
        refreshInherited()
    }

    // A line is “inherited” if one of its parents is checked
    function refreshInherited() {
        var stack = []
        for (var i = 0; i < treeModel.count; ++i) {
            var r = treeModel.get(i)
            var inh = r.depth > 0 ? (stack[r.depth - 1] === true) : false
            if (r.inherited !== inh)
                treeModel.setProperty(i, "inherited", inh)
            stack[r.depth] = inh || r.checked
        }
    }

    function clearAll() {
        selectedSet = {}
        for (var i = 0; i < treeModel.count; ++i)
            treeModel.setProperty(i, "checked", false)
        refreshInherited()
    }

    // Folders to exclude, excluding those already covered by a selected parent folder
    function selectedPaths() {
        var keys = Object.keys(selectedSet).sort()
        var out = []
        for (var i = 0; i < keys.length; ++i) {
            var covered = false
            for (var j = 0; j < out.length; ++j) {
                if (keys[i].indexOf(out[j] + "/") === 0) {
                    covered = true
                    break
                }
            }
            if (!covered)
                out.push(keys[i])
        }
        return out
    }

    ListModel {
        id: treeModel
    }

    Connections {
        target: win.backend
        function onFolderSizeReady(path, bytes) {
            win.sizeCache[path] = bytes
            for (var i = 0; i < treeModel.count; ++i) {
                if (treeModel.get(i).path === path) {
                    treeModel.setProperty(i, "sizeText", win.formatBytes(bytes))
                    break
                }
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 10

        Text {
            Layout.fillWidth: true
            text: qsTr("Check the folders you do NOT want to back up")
            font.pixelSize: 16
            font.bold: true
            color: "#2c3e50"
            wrapMode: Text.Wrap
        }

        Text {
            Layout.fillWidth: true
            text: qsTr("🔒 Folders required by the system cannot be excluded.")
            font.pixelSize: 11
            color: "#7f8c8d"
            wrapMode: Text.Wrap
        }

        CheckBox {
            id: hiddenBox
            text: qsTr("Show hidden folders")
            checked: win.showHidden
            // The window background is light (hard-coded): do not use the system palette,
            // which gives light text in a dark theme and makes the label unreadable.
            contentItem: Text {
                leftPadding: hiddenBox.indicator ? hiddenBox.indicator.width + hiddenBox.spacing : 0
                text: hiddenBox.text
                font: hiddenBox.font
                color: "#2c3e50"
                verticalAlignment: Text.AlignVCenter
            }
            onToggled: {
                win.showHidden = checked
                win.reload()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: "white"
            border.color: "#bdc3c7"
            radius: 4

            ListView {
                id: treeView
                anchors.fill: parent
                anchors.margins: 2
                clip: true
                model: treeModel
                ScrollBar.vertical: ScrollBar {}

                delegate: Rectangle {
                    width: treeView.width
                    height: 30
                    color: model.depth === 0 ? "#dfe6ec" : (index % 2 === 0 ? "#f7f9fb" : "#eef1f4")

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8 + model.depth * 22
                        anchors.rightMargin: 12
                        spacing: 6

                        Text {
                            Layout.preferredWidth: 18
                            horizontalAlignment: Text.AlignHCenter
                            text: model.hasChildren ? (model.expanded ? "▾" : "▸") : ""
                            color: "#2c3e50"
                            font.pixelSize: 14

                            MouseArea {
                                anchors.fill: parent
                                enabled: model.hasChildren
                                onClicked: win.toggleExpand(index)
                            }
                        }

                        CheckBox {
                            visible: model.excludable
                            checked: model.checked || model.inherited
                            enabled: model.excludable && !model.inherited
                            onToggled: win.setChecked(index, checked)
                        }

                        Text {
                            Layout.fillWidth: true
                            text: model.label + (model.locked ? "  🔒" : "")
                            elide: Text.ElideRight
                            color: model.excludable || model.depth === 0 ? "#2c3e50" : "#95a5a6"
                            font.pixelSize: 13
                            font.bold: model.depth === 0
                        }

                        Text {
                            text: model.sizeText
                            color: "#7f8c8d"
                            font.pixelSize: 12
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10

            Button {
                text: qsTr("Select none")
                onClicked: win.clearAll()
            }

            Item {
                Layout.fillWidth: true
            }

            Button {
                text: qsTr("Cancel")
                onClicked: win.close()
            }

            Button {
                text: qsTr("OK")
                onClicked: {
                    win.selectionAccepted(win.selectedPaths())
                    win.close()
                }
            }
        }
    }
}

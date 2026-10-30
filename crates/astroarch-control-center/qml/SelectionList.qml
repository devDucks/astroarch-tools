import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The tick list of the Data backup page, used by both profiles: groups of rows, a friendly
// name over the real path, a tri-state box per row, subfolders ticked one by one. The
// selection is a set of EXCLUSIONS (keys: item id, "item/child" or a path), so what is
// added later is included. Rows: see DataBackup.rows() (src/data_backup.rs); a row may
// also be `locked` (always included) or `raw` (its name is a folder name, not translated)
ColumnLayout {
    id: list

    property var groups: []
    property var excluded: ({})       // key -> true
    property var sizes: ({})          // key -> bytes
    property var openRows: ({})       // item id -> subfolders shown
    // A tick changed: the new exclusions (the owner keeps them)
    signal edited(var excluded)

    spacing: 0

    // The texts of data/data-profile.toml, for the translations (qsTr below takes them)
    readonly property var catalogTexts: [
        QT_TR_NOOP("KStars and Ekos"), QT_TR_NOOP("INDI"), QT_TR_NOOP("PHD2"),
        QT_TR_NOOP("Images and sessions"), QT_TR_NOOP("Kiosk desktop"),
        QT_TR_NOOP("KStars profiles and equipment"),
        QT_TR_NOOP("Without it, Ekos profiles, optical trains and the dark library list are lost."),
        QT_TR_NOOP("KStars dark library"),
        QT_TR_NOOP("Without it, all the dark frames have to be shot again."),
        QT_TR_NOOP("KStars settings"),
        QT_TR_NOOP("Without it, location, folders and Ekos options go back to the defaults."),
        QT_TR_NOOP("Plate-solving index files"),
        QT_TR_NOOP("The main index files are already included in the AstroArch image. Others can be downloaded again from KStars."),
        QT_TR_NOOP("INDI driver settings"),
        QT_TR_NOOP("Without it, every device has to be configured again."),
        QT_TR_NOOP("PHD2 profiles and settings"),
        QT_TR_NOOP("Without it, guiding profiles and calibration are lost."),
        QT_TR_NOOP("PHD2 dark library"),
        QT_TR_NOOP("Without it, guider darks and bad-pixel maps have to be made again."),
        QT_TR_NOOP("Images, sequences and targets"),
        QT_TR_NOOP("Kiosk settings and files"),
        QT_TR_NOOP("The kiosk has its own KStars, INDI and PHD2 settings, separate from the desktop."),
        QT_TR_NOOP("KStars profiles and settings"), QT_TR_NOOP("PHD2 profiles and dark library"),
        QT_TR_NOOP("Desktop and app settings"), QT_TR_NOOP("Pictures"), QT_TR_NOOP("Cache"),
        QT_TR_NOOP("Rebuilt automatically by the apps."), QT_TR_NOOP("Your home")
    ]

    function fmt(bytes) {
        return bytes === undefined || bytes === null ? "—" : Qt.locale().formattedDataSize(bytes)
    }
    // Texts of the catalog are translated; folder names are not
    function text(t, raw) {
        return !t ? "" : (raw ? t : qsTr(t))
    }

    // ------------------------------------------------- selection (tri-state)
    function isForced(item) {
        return !!item.locked || (!!item.neededBy && !list.excluded[item.neededBy])
    }
    function itemOn(item) {
        return list.isForced(item) || !list.excluded[item.id]
    }
    function childOn(item, child) {
        return list.itemOn(item) && !list.excluded[child.key]
    }
    function leftOut(item) {
        return list.itemOn(item) ? item.children.filter(c => list.excluded[c.key]).length : 0
    }
    function setExcluded(changes) {
        const e = Object.assign({}, list.excluded)
        for (const k in changes) {
            if (changes[k])
                e[k] = true
            else
                delete e[k]
        }
        list.edited(e)
    }
    // The main row ticks or unticks all its subfolders; a mixed one becomes fully ticked
    function toggleItem(item) {
        if (list.isForced(item))
            return
        const changes = {}
        for (const c of item.children)
            changes[c.key] = false
        changes[item.id] = list.itemOn(item) && list.leftOut(item) === 0
        list.setExcluded(changes)
    }
    // A subfolder ticked under an unticked row brings the row back with only that one;
    // unticking the last subfolder unticks the row too
    function toggleChild(item, child) {
        const changes = {}
        if (!list.itemOn(item)) {
            changes[item.id] = false
            for (const c of item.children)
                changes[c.key] = c.key !== child.key
        } else {
            changes[child.key] = !list.excluded[child.key]
            const allOff = item.children.every(c => c.key === child.key ? changes[c.key] : list.excluded[c.key])
            if (allOff && !list.isForced(item)) {
                for (const c of item.children)
                    changes[c.key] = false
                changes[item.id] = true
            }
        }
        list.setExcluded(changes)
    }
    // What a row copies; whole: what it would copy if fully ticked (shown when unticked)
    function itemBytes(item, whole) {
        if (item.children.length === 0)
            return list.sizes[item.id]
        let sum = 0
        let all = 0
        for (const c of item.children) {
            const b = list.sizes[c.key] || 0
            all += b
            if (whole || list.childOn(item, c))
                sum += b
        }
        // Loose files at the top of a folder whose subfolders are listed
        if (item.subfolders && list.sizes[item.id] !== undefined)
            sum += Math.max(0, list.sizes[item.id] - all)
        return sum
    }
    function selectedBytes() {
        let total = 0
        for (const g of list.groups)
            for (const it of g.items)
                if (list.itemOn(it))
                    total += list.itemBytes(it) || 0
        return total
    }
    function selectedCount() {
        let n = 0
        for (const g of list.groups)
            for (const it of g.items)
                if (list.itemOn(it) && it.exists)
                    n++
        return n
    }
    function itemNote(item) {
        if (!item.exists)
            return qsTr("Not on this system yet")
        if (item.locked)
            return qsTr("Needed by AstroArch: always included")
        if (list.isForced(item)) {
            const need = list.findItem(item.neededBy)
            return qsTr("Always included with the %1: it holds its list.").arg(need ? list.text(need.name) : item.neededBy)
        }
        if (!list.itemOn(item))
            return item.essential ? list.text(item.warn) : list.text(item.hint)
        const n = list.leftOut(item)
        return n > 0 ? qsTr("%1 of %2 subfolders left out").arg(n).arg(item.children.length) : ""
    }
    function findItem(id) {
        for (const g of list.groups)
            for (const it of g.items)
                if (it.id === id)
                    return it
        return null
    }

    Rectangle {
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

        Repeater {
            model: list.groups
            ColumnLayout {
                id: group
                required property var modelData
                required property int index
                Layout.fillWidth: true
                spacing: 0

                Rectangle {
                    visible: group.index > 0
                    Layout.fillWidth: true
                    implicitHeight: 1
                    color: "#e1e6ea"
                }
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 14
                    Layout.topMargin: 10
                    Layout.bottomMargin: 2
                    text: list.text(group.modelData.title)
                    color: "#5d6d7e"
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                    font.bold: true
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 0.5
                }

                Repeater {
                    model: group.modelData.items
                    ColumnLayout {
                        id: row
                        required property var modelData
                        readonly property var item: modelData
                        readonly property bool on: list.itemOn(item)
                        readonly property bool forced: list.isForced(item)
                        readonly property int out: list.leftOut(item)
                        readonly property bool expandable: item.children.length > 0
                        readonly property bool expanded: !!list.openRows[item.id]
                        readonly property string note: list.itemNote(item)
                        Layout.fillWidth: true
                        spacing: 0

                        RowLayout {
                            Layout.fillWidth: true
                            Layout.leftMargin: 10
                            Layout.rightMargin: 14
                            Layout.topMargin: 6
                            Layout.bottomMargin: 6
                            spacing: 10

                            TickBox {
                                tickState: !row.on ? Qt.Unchecked : (row.out > 0 ? Qt.PartiallyChecked : Qt.Checked)
                                forced: row.forced
                                enabled: !row.forced
                                Accessible.name: list.text(row.item.name, row.item.raw)
                                Accessible.description: row.note
                                onClicked: list.toggleItem(row.item)
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 1
                                RowLayout {
                                    spacing: 6
                                    Label {
                                        text: list.text(row.item.name, row.item.raw)
                                        color: row.on ? "#1f2d3a" : "#5d6d7e"
                                        font.bold: true
                                        font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.1
                                        MouseArea {
                                            anchors.fill: parent
                                            onClicked: list.toggleItem(row.item)
                                        }
                                    }
                                    Kirigami.Icon {
                                        visible: row.forced
                                        source: "object-locked"
                                        Layout.preferredWidth: 14
                                        Layout.preferredHeight: 14
                                        color: "#5d6d7e"
                                        isMask: true
                                    }
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: row.item.paths.join(", ")
                                    color: "#5d6d7e"
                                    font.family: "monospace"
                                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                                    elide: Text.ElideMiddle
                                }
                                Label {
                                    Layout.fillWidth: true
                                    visible: row.note !== ""
                                    text: row.note
                                    color: !row.on && row.item.essential ? "#8a4b00" : "#5d6d7e"
                                    font: Kirigami.Theme.smallFont
                                    wrapMode: Text.Wrap
                                }
                            }
                            AbstractButton {
                                id: expander
                                visible: row.expandable
                                focusPolicy: Qt.StrongFocus
                                implicitHeight: 36
                                implicitWidth: expanderRow.implicitWidth + 16
                                text: row.expanded ? qsTr("Hide subfolders") : qsTr("Choose subfolders")
                                Accessible.role: Accessible.Button
                                Accessible.name: text + " · " + list.text(row.item.name, row.item.raw)
                                Accessible.expandable: true
                                Accessible.expanded: row.expanded
                                Accessible.onPressAction: clicked()
                                onClicked: {
                                    const o = Object.assign({}, list.openRows)
                                    o[row.item.id] = !row.expanded
                                    list.openRows = o
                                }
                                background: Rectangle {
                                    radius: 4
                                    color: expander.hovered ? "#ffffff" : "transparent"
                                    border.color: expander.hovered || expander.visualFocus ? "#b9cddd" : "transparent"
                                }
                                contentItem: RowLayout {
                                    id: expanderRow
                                    spacing: 4
                                    Kirigami.Icon {
                                        source: row.expanded ? "go-down" : "go-next"
                                        Layout.preferredWidth: 14
                                        Layout.preferredHeight: 14
                                        color: "#1d6fa5"
                                        isMask: true
                                    }
                                    Label {
                                        text: expander.text
                                        color: "#1d6fa5"
                                        font: Kirigami.Theme.smallFont
                                    }
                                }
                            }
                            Label {
                                Layout.minimumWidth: 70
                                horizontalAlignment: Text.AlignRight
                                text: row.item.exists ? list.fmt(list.itemBytes(row.item, !row.on)) : "—"
                                color: "#34495e"
                            }
                        }

                        // Subfolders: fixed ones from the catalog, or the real folders
                        Repeater {
                            model: row.expanded ? row.item.children : []
                            Rectangle {
                                id: sub
                                required property var modelData
                                readonly property bool on: list.childOn(row.item, modelData)
                                Layout.fillWidth: true
                                implicitHeight: subRow.implicitHeight + 10
                                color: "#f8fafb"
                                RowLayout {
                                    id: subRow
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    anchors.verticalCenter: parent.verticalCenter
                                    anchors.leftMargin: 44
                                    anchors.rightMargin: 14
                                    spacing: 10
                                    TickBox {
                                        tickState: sub.on ? Qt.Checked : Qt.Unchecked
                                        Accessible.name: list.text(sub.modelData.name, row.item.raw || row.item.subfolders)
                                        onClicked: list.toggleChild(row.item, sub.modelData)
                                    }
                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 0
                                        Label {
                                            text: list.text(sub.modelData.name, row.item.raw || row.item.subfolders)
                                            color: sub.on ? "#1f2d3a" : "#5d6d7e"
                                            font.bold: true
                                            MouseArea {
                                                anchors.fill: parent
                                                onClicked: list.toggleChild(row.item, sub.modelData)
                                            }
                                        }
                                        Label {
                                            Layout.fillWidth: true
                                            text: sub.modelData.paths.join(", ")
                                            color: "#5d6d7e"
                                            font.family: "monospace"
                                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                                            elide: Text.ElideMiddle
                                        }
                                        Label {
                                            Layout.fillWidth: true
                                            visible: !sub.modelData.exists || sub.modelData.note !== ""
                                            text: !sub.modelData.exists ? qsTr("Not set up yet") : list.text(sub.modelData.note)
                                            color: "#5d6d7e"
                                            font: Kirigami.Theme.smallFont
                                            wrapMode: Text.Wrap
                                        }
                                    }
                                    Label {
                                        Layout.minimumWidth: 70
                                        horizontalAlignment: Text.AlignRight
                                        text: sub.modelData.exists ? list.fmt(list.sizes[sub.modelData.key]) : "—"
                                        color: "#34495e"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        }
    }
}

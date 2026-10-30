import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import AstroArchControlCenter

// The programs of the software installer, in the page: same list (software.toml) and
// same install / remove code (PackageManager, from astroarch-software-installer)
ColumnLayout {
    id: list

    property var items: []             // from PackageManager, grouped by category
    property string working: ""        // package being installed or removed
    property string message: ""
    property bool loaded: false

    spacing: 12

    // The categories of software.toml, for the translations (qsTr above takes them)
    readonly property var categoryTexts: [QT_TR_NOOP("Astronomy"), QT_TR_NOOP("Imaging"), QT_TR_NOOP("Tools")]

    // Categories in the order of software.toml
    readonly property var categories: {
        const seen = []
        for (const it of list.items)
            if (!seen.includes(it.category))
                seen.push(it.category)
        return seen
    }

    PackageManager {
        id: manager

        onPackagesLoaded: (json) => {
            try {
                list.items = JSON.parse(json).map(it => Object.assign(it, { category: it.category || qsTr("Other") }))
            } catch (e) {
                list.items = []
            }
            list.loaded = true
        }
        onLoadFailed: (message) => {
            list.message = qsTr("Cannot read the software list: %1").arg(message)
            list.loaded = true
        }
        onOperationFinished: (name, ok, installed, errorKind, detail) => {
            list.working = ""
            const it = list.items.find(i => i.name === name)
            const title = it ? it.title : name
            if (ok)
                list.message = installed ? qsTr("%1 installed.").arg(title) : qsTr("%1 removed.").arg(title)
            else if (errorKind === "auth")
                list.message = qsTr("Authentication cancelled or refused.")
            else
                list.message = qsTr("Could not change %1: %2").arg(title).arg(detail)
            manager.reload()
        }
    }

    Component.onCompleted: manager.reload()

    Label {
        visible: !list.loaded
        text: qsTr("Reading the list…")
        color: "#5d6d7e"
    }

    Repeater {
        model: list.categories

        ColumnLayout {
            id: category

            required property string modelData

            Layout.fillWidth: true
            spacing: 6

            Label {
                text: qsTr(category.modelData)
                color: "#4f5a64"
                font.bold: true
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: programs.implicitHeight
                radius: 8
                color: "#ffffff"
                border.color: "#d3dbe2"

                ColumnLayout {
                    id: programs
                    width: parent.width
                    spacing: 0

                    Repeater {
                        model: list.items.filter(it => it.category === category.modelData)

                        ColumnLayout {
                            id: program

                            required property var modelData
                            required property int index

                            Layout.fillWidth: true
                            spacing: 0

                            Rectangle {
                                visible: program.index > 0
                                Layout.fillWidth: true
                                implicitHeight: 1
                                color: "#e1e6ea"
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                Layout.margins: 12
                                spacing: 16

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2
                                    Label {
                                        Layout.fillWidth: true
                                        text: program.modelData.title
                                        color: "#1f2d3a"
                                        font.bold: true
                                        wrapMode: Text.Wrap
                                    }
                                    Label {
                                        Layout.fillWidth: true
                                        text: program.modelData.description
                                        color: "#5d6d7e"
                                        font: Kirigami.Theme.smallFont
                                        wrapMode: Text.Wrap
                                    }
                                }

                                CardButton {
                                    readonly property bool busy: list.working === program.modelData.name
                                    text: busy ? (program.modelData.installed ? qsTr("Removing…") : qsTr("Installing…"))
                                         : program.modelData.installed ? qsTr("Remove")
                                         : program.modelData.available ? qsTr("Install")
                                         : qsTr("Not available")
                                    enabled: !manager.busy && list.working === ""
                                             && (program.modelData.installed || program.modelData.available)
                                    Accessible.name: text + " · " + program.modelData.title
                                    onClicked: {
                                        list.working = program.modelData.name
                                        list.message = ""
                                        manager.apply(program.modelData.name, !program.modelData.installed, program.modelData.aur)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: list.message !== ""
        type: Kirigami.MessageType.Information
        text: list.message
    }
}

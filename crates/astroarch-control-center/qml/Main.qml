import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import AstroArchControlCenter
import QtQuick.Window

ApplicationWindow {
    id: root

    width: 780
    height: 600
    minimumWidth: 520
    minimumHeight: 460
    visible: true
    title: qsTr("AstroArch Control Center")
    color: "#0d1117"

    Component.onCompleted: {
        window.window.windowHandled.setProperty("windowIcon", "file:///home/astronaut/.astroarch/assets/icons/novnc-16x16.png")
    }

    Launcher {
        id: launcher
    }

    // To add a button later: add one entry here, nothing to change in Rust.
    // terminal: true  -> command runs in a separate terminal window
    // terminal: false -> command is started as a standalone application
    readonly property var entries: [
        {
            glyph: "\u27F3",
            accent: "#4f8cff",
            title: qsTr("System update"),
            subtitle: qsTr("Run update-astroarch in a terminal"),
            command: "update-astroarch",
            terminal: true
        },
        {
            glyph: "\u2328",
            accent: "#3fb950",
            title: qsTr("Language & keyboard"),
            subtitle: qsTr("Configure language, location and keyboard"),
            command: "astroarch-locale-module",
            terminal: false
        },
        {
            glyph: "\u2699",
            accent: "#d29922",
            title: qsTr("Services"),
            subtitle: qsTr("Enable, disable and monitor system services"),
            command: "astroarch-sys-manager",
            terminal: false
        },
        {
            glyph: "\u27F2",
            accent: "#39c5bb",
            title: qsTr("Backup"),
            subtitle: qsTr("Back up and restore the AstroArch system"),
            command: "astroarch-backup",
            terminal: false
        },
        {
            glyph: "\u2604",
            accent: "#f85149",
            title: qsTr("Astro monitor"),
            subtitle: qsTr("Profile Backup Tool for KStars/Indi/PHD2"),
            command: "astromonitor",
            terminal: true
        },
        {
            glyph: "\u21E9",
            accent: "#a371f7",
            title: qsTr("Software"),
            subtitle: qsTr("Choose astronomy and other software to install"),
            command: "astroarch-software-installer",
            terminal: false
        }
    ]

    function run(entry) {
        const ok = launcher.launch(entry.command, entry.terminal)
        showToast(ok
                  ? qsTr("Launching: %1").arg(entry.title)
                  : qsTr("Unable to launch \"%1\" (command not found or failed)").arg(entry.command),
                  !ok)
    }

    function showToast(message, isError) {
        toastLabel.text = message
        toast.isError = isError
        toast.opacity = 1
        toastTimer.restart()
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 24
        spacing: 20

        ColumnLayout {
            spacing: 2

            Label {
                text: qsTr("AstroArch Control Center")
                color: "#e6edf3"
                font.pixelSize: 26
                font.bold: true
            }
            Label {
                text: qsTr("Maintain and configure your AstroArch system")
                color: "#8b98a9"
                font.pixelSize: 14
            }
        }

        GridLayout {
            id: grid
            Layout.fillWidth: true
            Layout.fillHeight: true
            columns: root.width < 640 ? 1 : 2
            columnSpacing: 16
            rowSpacing: 16

            Repeater {
                model: root.entries

                LauncherButton {
                    required property var modelData
                    required property int index

                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.columnSpan: (grid.columns === 2
                                        && index === root.entries.length - 1
                                        && root.entries.length % 2 === 1) ? 2 : 1

                    glyph: modelData.glyph
                    accent: modelData.accent
                    heading: modelData.title
                    subtitle: modelData.subtitle
                    onClicked: root.run(modelData)
                }
            }
        }
    }

    // Status message at the bottom of the window
    Rectangle {
        id: toast

        property bool isError: false

        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 20
        width: Math.min(parent.width - 48, toastLabel.implicitWidth + 32)
        height: toastLabel.implicitHeight + 20
        radius: 8
        color: isError ? "#5c1f1f" : "#1f3d2a"
        border.color: isError ? "#f85149" : "#3fb950"
        opacity: 0
        visible: opacity > 0

        Behavior on opacity {
            NumberAnimation { duration: 200 }
        }

        Label {
            id: toastLabel
            anchors.centerIn: parent
            width: parent.width - 32
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: "#e6edf3"
            font.pixelSize: 13
        }

        Timer {
            id: toastTimer
            interval: 4000
            onTriggered: toast.opacity = 0
        }
    }
}

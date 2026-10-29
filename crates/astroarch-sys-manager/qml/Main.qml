import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import AstroArchSysManager

ApplicationWindow {

    visible: true
    width: 700
    height: 600
    title: qsTr("AstroArch - Service Configuration")
    color: "#1e1e1e"

    readonly property color bgCard: "#2d2d2d"
    readonly property color bgSection: "#252525"
    readonly property color textMain: "#ffffff"
    readonly property color textLabel: "#cccccc"
    readonly property color accent: "#3daee9"
    readonly property color statusActive: "#2ec27e"
    readonly property color statusInactive: "#ed333b"
    readonly property color statusDisabled: "#808080"
    readonly property color statusWarning: "#e67e22"

    ServiceManager {
        id: serviceManager
        Component.onCompleted: serviceManager.start_monitoring()
    }

    // Reusable component for each service line
    component ServiceRow: RowLayout {
        property string title: ""
        property string status: ""
        property string plugin: ""
        property string cmdOn: ""
        property string cmdOff: ""
        property string cmdRemove: ""
        property bool isInstalled: true

        spacing: 10
        Layout.fillWidth: true
        Layout.leftMargin: 0
        Layout.rightMargin: 0

        Text {
            text: title
            color: textMain
            font.pixelSize: 12
            font.weight: Font.Medium
            Layout.preferredWidth: 220
            Layout.alignment: Qt.AlignVCenter
            opacity: parent.isInstalled ? 1.0 : 0.5
        }

        Text {
            property string s: status.toLowerCase()
            text: {
                if (!parent.isInstalled) {
                    return qsTr("● Deactivated");
                }

                var s = status.toLowerCase();
                if (s === "active") return qsTr("● Active");
                if (s === "stopping") return qsTr("● Stopped");
                if (s === "enabled") return qsTr("● Enabled");
                return qsTr("○ Inactive");
            }
            color: {
                if (!parent.isInstalled) {
                    return statusDisabled;
                }

                var s = status.toLowerCase();
                if (s === "active") return statusActive;
                if (s === "stopping") return statusWarning;
                if (s === "enabled") return statusDisabled;
                return statusInactive;
            }
            font.bold: true
            font.pixelSize: 11
            Layout.preferredWidth: 180
            Layout.alignment: Qt.AlignVCenter
        }

        Button {
            text: {
                if (!parent.isInstalled) {
                    return qsTr("Install");
                }
                var s = status.toLowerCase();
                if (s === "active" || s === "stopping") {
                    return qsTr("Stop");
                }
                return qsTr("Start");
            }
            highlighted: !parent.isInstalled || status.toLowerCase() !== "active"
            Layout.preferredWidth: 105
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            font.pixelSize: 10
            onClicked: serviceManager.toggle_service(plugin, cmdOn, cmdOff, status.toLowerCase() === "active")
        }

        Button {
            text: qsTr("Uninstall")
            visible: cmdRemove !== ""
            Layout.preferredWidth: 105
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            font.pixelSize: 10
            palette.buttonText: "#ed333b"
            onClicked: serviceManager.remove_service(plugin, cmdRemove)
        }

        // Blank space for services without an "Uninstall" button
        Rectangle {
            visible: cmdRemove === ""
            Layout.preferredWidth: 100
            Layout.preferredHeight: 32
            color: "transparent"
        }
    }

    Flickable {
        anchors.fill: parent
        anchors.margins: 0
        contentHeight: mainCol.implicitHeight
        clip: true

        ColumnLayout {
            id: mainCol
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 18
            spacing: 18
            width: parent.width - 36

            // === Main Title ===
            Text {
                text: qsTr("Configuring AstroArch Services")
                color: textMain
                font.pixelSize: 18
                font.weight: Font.Bold
                Layout.alignment: Qt.AlignHCenter
                Layout.bottomMargin: 8
            }

            // === SYSTEM SERVICES ===
            Text {
                text: qsTr("System Services")
                color: "#888888"
                font.pixelSize: 10
                font.capitalization: Font.AllUppercase
                font.weight: Font.Bold
                Layout.topMargin: 6
            }

            Rectangle {
                color: bgSection
                Layout.fillWidth: true
                radius: 6
                implicitHeight: sysCol.implicitHeight + 24
                border.color: "#404040"
                border.width: 1

                ColumnLayout {
                    id: sysCol
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 10

                    ServiceRow {
                        title: qsTr("Bluetooth")
                        status: serviceManager.bluetoothStatus
                        plugin: "/home/astronaut/.astroarch/plugins/bluetooth/bluetooth.plugin.zsh"
                        cmdOn: "bluetooth_on"
                        cmdOff: "bluetooth_off"
                        cmdRemove: "bluetooth_remove"
                        isInstalled: serviceManager.bluetoothInstalled
                    }
                    ServiceRow {
                        title: qsTr("FTP Server (vsftpd)")
                        status: serviceManager.vsftpdStatus
                        plugin: "/home/astronaut/.astroarch/plugins/ftp/ftp.plugin.zsh"
                        cmdOn: "ftp_on"
                        cmdOff: "ftp_off"
                        cmdRemove: "ftp_remove"
                        isInstalled: serviceManager.vsftpdInstalled
                    }
                    ServiceRow {
                        title: qsTr("GPS Daemon (gpsd)")
                        status: serviceManager.gpsdStatus
                        plugin: "/home/astronaut/.astroarch/plugins/gps/gps.plugin.zsh"
                        cmdOn: "gps_on"
                        cmdOff: "gps_off"
                        cmdRemove: ""
                        isInstalled: serviceManager.gpsdInstalled
                    }
                }
            }

            // === HARDWARE OPTIMIZATIONS (RASPBERRY PI) ===
            Text {
                text: qsTr("Hardware optimizations")
                color: "#888888"
                font.pixelSize: 10
                font.capitalization: Font.AllUppercase
                font.weight: Font.Bold
                Layout.topMargin: 6
            }

            Rectangle {
                color: bgSection
                Layout.fillWidth: true
                radius: 6
                implicitHeight: rpiRow.implicitHeight + 24
                border.color: "#404040"
                border.width: 1

                RowLayout {
                    id: rpiRow
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 10

                    Text {
                        text: qsTr("Maximum USB current (Pi 5) :")
                        color: textMain
                        font.pixelSize: 12
                        font.weight: Font.Medium
                        Layout.preferredWidth: 195
                        Layout.alignment: Qt.AlignVCenter
                    }

                    Text {
                        property string s: serviceManager.rpiStatus.toLowerCase()
                        text: {
                            if (s === "active") return qsTr("● Active");
                            return qsTr("○ Inactive");
                        }
                        color: s === "active" ? statusActive : statusInactive
                        font.bold: true
                        font.pixelSize: 11
                        Layout.preferredWidth: 160
                        Layout.alignment: Qt.AlignVCenter
                    }

                    Button {
                        text: {
                            if (serviceManager.rpiStatus.toLowerCase() === "active") {
                                return qsTr("Already configured");
                            }
                            return qsTr("Optimize");
                        }
                        highlighted: serviceManager.rpiStatus.toLowerCase() !== "active"
                        enabled: serviceManager.rpiStatus.toLowerCase() !== "active"
                        Layout.preferredWidth: 120
                        Layout.preferredHeight: 32
                        Layout.alignment: Qt.AlignVCenter
                        font.pixelSize: 10
                        onClicked: serviceManager.run_rpi_optimization()
                    }

                    Rectangle {
                        Layout.preferredWidth: 65
                        Layout.preferredHeight: 32
                        color: "transparent"
                    }
                }
            }

            Item {
                Layout.fillHeight: true
                Layout.minimumHeight: 10
            }
        }
    }
}

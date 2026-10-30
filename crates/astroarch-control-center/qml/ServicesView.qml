import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import AstroArchControlCenter

// The Services page: the state and the commands of astroarch-sys-manager (its
// ServiceManager, compiled in from crates/astroarch-sys-manager: the same code, not a copy).
// The commands are the functions of the AstroArch zsh plugins, run in a terminal
ColumnLayout {
    id: view

    property var probes: ({})
    property string message: ""

    // The AstroArch plugins folder of the main user (probe plugins-dir)
    readonly property string plugins: probes["plugins-dir"] !== undefined && probes["plugins-dir"].ok
                                      ? probes["plugins-dir"].value : ""
    // "none" on anything but a Raspberry Pi 5 (probe usb-current)
    readonly property bool pi5: probes["usb-current"] === undefined || !probes["usb-current"].ok
                                || probes["usb-current"].value !== "none"

    spacing: 12

    ServiceManager {
        id: services
        Component.onCompleted: services.start_monitoring()
    }

    function stateText(installed, status) {
        if (!installed)
            return qsTr("Not installed")
        if (status === "active")
            return qsTr("On")
        if (status === "enabled")
            return qsTr("Off, starts with the system")
        return qsTr("Off")
    }

    // A service row: name, state, Turn on / Turn off (Install when missing), Remove
    component ServiceRow: ColumnLayout {
        id: row

        required property string name
        required property string status
        required property bool installed
        property string plugin: ""
        property string cmdOn: ""
        property string cmdOff: ""
        property string cmdRemove: ""
        property bool first: false
        readonly property bool active: status === "active"

        Layout.fillWidth: true
        spacing: 0

        Rectangle {
            visible: !row.first
            Layout.fillWidth: true
            implicitHeight: 1
            color: "#e1e6ea"
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 12
            spacing: 16

            Label {
                Layout.preferredWidth: 130
                text: row.name
                color: "#4f5a64"
                font.bold: true
                wrapMode: Text.Wrap
            }
            Label {
                Layout.fillWidth: true
                text: view.stateText(row.installed, row.status)
                color: "#1f2d3a"
                wrapMode: Text.Wrap
            }
            CardButton {
                text: !row.installed ? qsTr("Install") : (row.active ? qsTr("Turn off") : qsTr("Turn on"))
                enabled: view.plugins !== ""
                Accessible.name: text + " · " + row.name
                onClicked: {
                    services.toggle_service(view.plugins + "/" + row.plugin, row.cmdOn, row.cmdOff, row.active)
                    view.message = qsTr("“%1” opens in a terminal, which may ask for your password.").arg(row.name)
                }
            }
            CardButton {
                visible: row.installed && row.cmdRemove !== ""
                text: qsTr("Remove")
                enabled: view.plugins !== ""
                Accessible.name: text + " · " + row.name
                onClicked: {
                    services.remove_service(view.plugins + "/" + row.plugin, row.cmdRemove)
                    view.message = qsTr("“%1” opens in a terminal, which may ask for your password.").arg(row.name)
                }
            }
        }
    }

    Rectangle {
        Layout.fillWidth: true
        implicitHeight: rows.implicitHeight
        radius: 8
        color: "#ffffff"
        border.color: "#d3dbe2"

        ColumnLayout {
            id: rows
            width: parent.width
            spacing: 0

            ServiceRow {
                first: true
                name: qsTr("Bluetooth")
                status: services.bluetoothStatus
                installed: services.bluetoothInstalled
                plugin: "bluetooth/bluetooth.plugin.zsh"
                cmdOn: "bluetooth_on"
                cmdOff: "bluetooth_off"
                cmdRemove: "bluetooth_remove"
            }
            ServiceRow {
                name: qsTr("File transfer (FTP)")
                status: services.vsftpdStatus
                installed: services.vsftpdInstalled
                plugin: "ftp/ftp.plugin.zsh"
                cmdOn: "ftp_on"
                cmdOff: "ftp_off"
                cmdRemove: "ftp_remove"
            }

            // USB power of the Raspberry Pi 5: can only be turned on
            ColumnLayout {
                visible: view.pi5
                Layout.fillWidth: true
                spacing: 0

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 1
                    color: "#e1e6ea"
                }
                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 12
                    spacing: 16

                    Label {
                        Layout.preferredWidth: 130
                        text: qsTr("USB power")
                        color: "#4f5a64"
                        font.bold: true
                        wrapMode: Text.Wrap
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Label {
                            Layout.fillWidth: true
                            text: services.rpiStatus === "active" ? qsTr("On") : qsTr("Off")
                            color: "#1f2d3a"
                        }
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Full current on the USB ports of the Raspberry Pi 5, after a restart")
                            color: "#5d6d7e"
                            font: Kirigami.Theme.smallFont
                            wrapMode: Text.Wrap
                        }
                    }
                    CardButton {
                        visible: services.rpiStatus !== "active"
                        enabled: view.plugins !== ""
                        text: qsTr("Turn on")
                        Accessible.name: text + " · " + qsTr("USB power")
                        onClicked: {
                            // The main user's plugin (run_rpi_optimization has a fixed home)
                            services.toggle_service(view.plugins + "/power_max_current/power_max_current.plugin.zsh",
                                                    "power_max_current", "", false)
                            view.message = qsTr("“%1” opens in a terminal, which may ask for your password.").arg(qsTr("USB power"))
                        }
                    }
                }
            }
        }
    }

    Label {
        Layout.fillWidth: true
        visible: view.plugins === ""
        text: qsTr("The AstroArch plugins were not found: the services cannot be changed from here.")
        color: "#5d6d7e"
        wrapMode: Text.Wrap
    }

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: view.message !== ""
        type: Kirigami.MessageType.Information
        text: view.message
    }
}

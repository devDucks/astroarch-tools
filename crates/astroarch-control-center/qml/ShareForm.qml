import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// "Connect a network folder": a share of a NAS or another computer, kept across restarts
// (helper nas-add of the backup tool, through DataBackup.connectShare). Used by the Data
// backup page and the Restore page; the owner listens to the manager's shareChanged and
// gives its message, shown under Connect
Rectangle {
    id: form

    property var manager
    property string kind: "smb"
    property string message: ""
    property bool warning: false
    // The message box, for the owner to scroll to
    readonly property alias messageItem: shareMessage
    readonly property bool ready: !!form.manager && !form.manager.busy && shareServer.text.trim() !== "" && shareName.text.trim() !== ""
    signal cancelled()
    signal submitted()

    // What a failed connectShare / removeShare says (DataBackup's shareChanged codes)
    function errorText(code) {
        return ({ auth: qsTr("Authentication cancelled or refused."),
                  server: qsTr("The server name is not valid."),
                  share: qsTr("The share name is not valid: letters, digits, spaces and . _ $ - only."),
                  export: qsTr("The export path is not valid: it starts with / (e.g. /export/backups)."),
                  user: qsTr("The user name is not valid."),
                  password: qsTr("The password cannot hold a line break.") })[code]
               || qsTr("Could not connect the network folder: %1").arg(code)
    }
    function clearPassword() {
        sharePassword.text = ""
    }
    // Enter in a field (Main.qml calls accepted()) or the Connect button
    function connect() {
        if (!form.ready)
            return
        form.submitted()
        form.manager.connectShare(JSON.stringify({ kind: form.kind, server: shareServer.text,
            share: shareName.text, user: form.kind === "smb" ? shareUser.text : "",
            password: form.kind === "smb" ? sharePassword.text : "" }))
    }

    Layout.fillWidth: true
    implicitHeight: box.implicitHeight + 28
    radius: 8
    color: "#ffffff"
    border.color: "#d3dbe2"

    ColumnLayout {
        id: box
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 14
        spacing: 10
        Label {
            text: qsTr("Connect a network folder")
            color: "#1f2d3a"
            font.bold: true
        }
        RowLayout {
            spacing: 6
            Repeater {
                model: [{ id: "smb", text: qsTr("Windows / Samba (SMB)") }, { id: "nfs", text: qsTr("NFS") }]
                CardButton {
                    required property var modelData
                    text: modelData.text
                    checked: form.kind === modelData.id
                    Accessible.role: Accessible.RadioButton
                    Accessible.checked: checked
                    onClicked: form.kind = modelData.id
                }
            }
        }
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 12
            rowSpacing: 8
            Label { text: qsTr("Server"); color: "#4f5a64"; font.bold: true }
            CardField { id: shareServer; onAccepted: form.connect(); Layout.fillWidth: true; placeholderText: qsTr("e.g. nas.local or 192.168.1.10"); Accessible.name: qsTr("Server") }
            Label { text: form.kind === "smb" ? qsTr("Share") : qsTr("Export path"); color: "#4f5a64"; font.bold: true }
            CardField {
                id: shareName
                onAccepted: form.connect()
                Layout.fillWidth: true
                placeholderText: form.kind === "smb" ? qsTr("e.g. backups") : qsTr("e.g. /export/backups")
                Accessible.name: form.kind === "smb" ? qsTr("Share") : qsTr("Export path")
            }
            Label { visible: form.kind === "smb"; text: qsTr("User"); color: "#4f5a64"; font.bold: true }
            CardField { id: shareUser; onAccepted: form.connect(); visible: form.kind === "smb"; Layout.fillWidth: true; placeholderText: qsTr("Empty for a guest share"); Accessible.name: qsTr("User") }
            Label { visible: form.kind === "smb"; text: qsTr("Password"); color: "#4f5a64"; font.bold: true }
            CardField { id: sharePassword; onAccepted: form.connect(); visible: form.kind === "smb"; Layout.fillWidth: true; echoMode: TextInput.Password; Accessible.name: qsTr("Password") }
        }
        // About the user and password: none for NFS
        Label {
            Layout.fillWidth: true
            visible: form.kind === "smb"
            text: qsTr("The password is kept readable by the administrator only. The folder is mounted when a backup needs it, and the Pi starts without it. Your password may be asked.")
            color: "#5d6d7e"
            font: Kirigami.Theme.smallFont
            wrapMode: Text.Wrap
        }
        RowLayout {
            Layout.fillWidth: true
            Item { Layout.fillWidth: true }
            CardButton {
                text: qsTr("Cancel")
                onClicked: {
                    form.clearPassword()
                    form.cancelled()
                }
            }
            CardButton {
                highlighted: true
                enabled: form.ready
                text: !!form.manager && form.manager.busy ? qsTr("Connecting…") : qsTr("Connect")
                onClicked: form.connect()
            }
        }
        Kirigami.InlineMessage {
            id: shareMessage
            Layout.fillWidth: true
            visible: form.message !== ""
            type: form.warning ? Kirigami.MessageType.Warning : Kirigami.MessageType.Information
            text: form.message
        }
    }
}

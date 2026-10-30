import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import AstroArchControlCenter

// The Image storage page, below its rows: move the shared folder to a drive and back.
// Where the folder is comes from the probes of the page (astro-data, astro-data-place);
// the work is done as root by astroarch-storage-helper, through StorageManager
ColumnLayout {
    id: view

    property var probes: ({})
    property var fsName: type => type      // display name of a file system (Main.qml)
    signal systemChanged()                 // the folder moved: read the probes again

    property var drives: []
    property string drivesJson: ""
    property bool ownChange: false          // the next list follows this page's own change
    property bool listed: false
    property string confirming: ""         // "move", "back" or "forget", waiting for a yes
    property var chosen: null              // the drive of "move"
    property string message: ""
    property bool failed: false
    // A confirmation is shown, with its button: the page's primary one (Main.qml)
    readonly property bool ownsPrimary: view.confirming !== ""

    // "internal", "drive|name|type" or "missing|name|type"; "path|used KiB|free KiB"
    readonly property var placeParts: view.probeParts("astro-data-place")
    readonly property string place: placeParts.length > 0 ? placeParts[0] : ""
    readonly property string driveName: placeParts[1] || ""
    readonly property real usedBytes: (parseInt(view.probeParts("astro-data")[1]) || 0) * 1024

    spacing: 12

    function probeParts(name) {
        const result = view.probes[name]
        return result !== undefined && result.ok ? result.value.split("|") : []
    }

    function size(bytes) {
        return Qt.locale().formattedDataSize(bytes)
    }

    function run(action, device) {
        view.confirming = ""
        view.message = ""
        manager.run(action, device || "")
    }

    function doneText(action) {
        return ({
            move: qsTr("Done: the folder is on the drive now. Keep the drive plugged in."),
            back: qsTr("Done: the folder is back on the internal disk. The files are also still on the drive."),
            reconnect: qsTr("The drive is connected again."),
            forget: qsTr("AstroArch no longer uses the drive. Your files are still on it, in astroarch/astro-data.")
        })[action] || ""
    }

    // The helper's error codes (scripts/astroarch-storage-helper.sh)
    function errorText(code, detail) {
        switch (code) {
        case "auth":
            return qsTr("Not done: this needs an administrator password. If no password window opened, do it from the main desktop.")
        case "busy":
            return qsTr("Close these programs first, on both desktops: %1.").arg(detail.split(",").join(", "))
        case "space": {
            const kib = detail.split(" ")
            return qsTr("Not enough room: %1 needed, %2 free.").arg(view.size(kib[0] * 1024)).arg(view.size(kib[1] * 1024))
        }
        case "drive-missing":
            return qsTr("The drive is not connected. Plug it in, then try again.")
        case "drive-busy":
            return qsTr("Files on the drive are open in another program. Close them, then try again.")
        case "system":
            return qsTr("This drive holds a system: choose another one.")
        case "copy":
            return qsTr("The copy stopped, nothing was deleted: %1").arg(detail)
        case "missing":
            return qsTr("The image storage helper is not installed (%1).").arg(detail)
        }
        return qsTr("It did not work: %1").arg(detail || code)
    }

    function stageText() {
        switch (manager.stage) {
        case "auth":
            return qsTr("Waiting for the password…")
        case "prepare":
            return qsTr("Checking the drive…")
        case "copy":
            return manager.progress >= 0 ? qsTr("Copying the files… %1%").arg(manager.progress) : qsTr("Copying the files…")
        }
        return qsTr("Switching to the new place…")
    }

    // What a drive of the list can be used for
    function driveNote(drive) {
        const details = [view.fsName(drive.fstype), view.size(drive.size)]
        if (drive.free >= 0)
            details.push(qsTr("%1 free").arg(view.size(drive.free)))
        const notes = {
            fat: qsTr("FAT32 cannot keep files over 4 GB, such as long planetary videos"),
            readonly: qsTr("Read-only"),
            unsupported: qsTr("This format cannot be used")
        }
        return details.join(" · ") + (notes[drive.state] ? "\n" + notes[drive.state] : "")
    }

    // The drive may already have an astroarch/astro-data folder: the files are merged into it
    function mergeNote(drive) {
        if (drive.hasData === true)
            return " " + qsTr("The drive already has an astroarch/astro-data folder: your files are merged into it, and nothing on the drive is deleted.")
        if (drive.hasData === null)
            return " " + qsTr("If the drive already has an astroarch/astro-data folder, your files are merged into it: nothing on the drive is deleted.")
        return ""
    }

    function usable(drive) {
        return (drive.state === "ok" || drive.state === "fat") && (drive.free < 0 || drive.free > view.usedBytes)
    }

    StorageManager {
        id: manager

        // A list rebuilt only when it changed keeps the keyboard focus. Partitions of
        // another system (ALARM_*) are not in it: they are never offered (drives.rs)
        onDrivesListed: (json) => {
            const own = view.ownChange
            view.ownChange = false
            if (json === view.drivesJson)
                return
            // A drive plugged in, out, mounted or unmounted outside this page: the page is
            // read again, and an old message goes
            if (view.listed && !own) {
                view.message = ""
                view.systemChanged()
            }
            view.listed = true
            view.drivesJson = json
            try {
                view.drives = JSON.parse(json)
            } catch (e) {
                view.drives = []
            }
        }
        onFinished: (action, code, detail) => {
            view.ownChange = true
            view.failed = code !== ""
            view.message = code === "" ? view.doneText(action) : view.errorText(code, detail)
            view.systemChanged()
            manager.list_drives()
        }
    }

    // The list follows the drives plugged in and out, while the page is shown
    Timer {
        interval: 4000
        repeat: true
        triggeredOnStart: true
        running: view.visible && !manager.busy
        onTriggered: manager.list_drives()
    }

    // ------------------------------------------------------------- working
    ColumnLayout {
        visible: manager.busy
        Layout.fillWidth: true
        spacing: 6
        Label {
            Layout.fillWidth: true
            text: view.stageText()
            color: "#1f2d3a"
            wrapMode: Text.Wrap
        }
        // Drawn here, as the buttons of the card: the copy's percentage, or a block going
        // back and forth while it is not known
        Rectangle {
            id: track
            Layout.fillWidth: true
            implicitHeight: 8
            radius: 4
            color: "#dde3e8"
            clip: true
            Rectangle {
                height: parent.height
                radius: 4
                color: "#1d6fa5"
                width: manager.progress >= 0 ? parent.width * manager.progress / 100 : parent.width / 4
                SequentialAnimation on x {
                    running: manager.busy && manager.progress < 0
                    loops: Animation.Infinite
                    NumberAnimation { from: 0; to: track.width * 3 / 4; duration: 900; easing.type: Easing.InOutQuad }
                    NumberAnimation { from: track.width * 3 / 4; to: 0; duration: 900; easing.type: Easing.InOutQuad }
                }
                onWidthChanged: if (manager.progress >= 0) x = 0
            }
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("Leave this window open until it is finished.")
            color: "#5d6d7e"
            font: Kirigami.Theme.smallFont
            wrapMode: Text.Wrap
        }
    }

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: view.message !== "" && !manager.busy
        type: view.failed ? Kirigami.MessageType.Error : Kirigami.MessageType.Positive
        text: view.message
    }

    // ------------------------------------------- on the internal disk: the drives
    ColumnLayout {
        visible: view.place === "internal" && !manager.busy
        Layout.fillWidth: true
        spacing: 6

        Label {
            Layout.fillWidth: true
            text: qsTr("Move to a drive")
            color: "#4f5a64"
            font.bold: true
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("Copy the folder to a USB disk or an SSD and keep it there. The drive is not formatted: the folder goes in astroarch/astro-data on it.")
            color: "#34495e"
            wrapMode: Text.Wrap
        }
        Label {
            Layout.fillWidth: true
            visible: view.listed && view.drives.length === 0
            text: qsTr("No drive found. Plug in a USB disk or an SSD: it shows up here.")
            color: "#5d6d7e"
            wrapMode: Text.Wrap
        }

        Rectangle {
            visible: view.drives.length > 0
            Layout.fillWidth: true
            implicitHeight: driveRows.implicitHeight
            radius: 8
            color: "#ffffff"
            border.color: "#d3dbe2"

            ColumnLayout {
                id: driveRows
                width: parent.width
                spacing: 0

                Repeater {
                    model: view.drives

                    ColumnLayout {
                        id: driveRow

                        required property var modelData
                        required property int index

                        Layout.fillWidth: true
                        spacing: 0

                        Rectangle {
                            visible: driveRow.index > 0
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
                                    text: driveRow.modelData.label
                                    color: "#1f2d3a"
                                    font.bold: true
                                    wrapMode: Text.Wrap
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: view.driveNote(driveRow.modelData)
                                    color: "#5d6d7e"
                                    font: Kirigami.Theme.smallFont
                                    wrapMode: Text.Wrap
                                }
                            }
                            CardButton {
                                text: qsTr("Move here…")
                                enabled: view.usable(driveRow.modelData)
                                Accessible.name: text + " · " + driveRow.modelData.label
                                onClicked: {
                                    view.message = ""
                                    view.chosen = driveRow.modelData
                                    view.confirming = "move"
                                }
                            }
                        }
                        Loader {
                            Layout.fillWidth: true
                            Layout.leftMargin: 12
                            Layout.rightMargin: 12
                            Layout.bottomMargin: 12
                            active: view.confirming === "move" && view.chosen !== null
                                    && view.chosen.device === driveRow.modelData.device
                            visible: active
                            sourceComponent: confirmation
                        }
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------- on a drive
    ColumnLayout {
        visible: view.place === "drive" && !manager.busy
        Layout.fillWidth: true
        spacing: 6

        Label {
            Layout.fillWidth: true
            text: qsTr("Keep the drive plugged in. Without it, programs cannot save in this folder, and nothing is saved on the internal disk by mistake. Plugged in again, it is used again by itself.")
            color: "#34495e"
            wrapMode: Text.Wrap
        }
        CardButton {
            text: qsTr("Move back to the internal disk…")
            onClicked: {
                view.message = ""
                view.confirming = "back"
            }
        }
        Loader {
            Layout.fillWidth: true
            active: view.confirming === "back"
            visible: active
            sourceComponent: confirmation
        }
    }

    // ---------------------------------------------------- drive not connected
    ColumnLayout {
        visible: view.place === "missing" && !manager.busy
        Layout.fillWidth: true
        spacing: 8

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: true
            type: Kirigami.MessageType.Warning
            text: qsTr("The drive “%1” is not connected. Until it is back, programs cannot save in this folder. Plug it in: it is used again by itself.").arg(view.driveName)
        }
        RowLayout {
            spacing: 8
            CardButton {
                text: qsTr("Connect again")
                onClicked: view.run("reconnect")
            }
            CardButton {
                text: qsTr("Stop using the drive…")
                onClicked: {
                    view.message = ""
                    view.confirming = "forget"
                }
            }
        }
        Loader {
            Layout.fillWidth: true
            active: view.confirming === "forget"
            visible: active
            sourceComponent: confirmation
        }
    }

    // ------------------------------------------------------ confirmation
    // Shown next to the button that asked for it
    Component {
        id: confirmation

        ColumnLayout {
            spacing: 8

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: true
                type: Kirigami.MessageType.Warning
                text: view.confirming === "move" && view.chosen
                      ? qsTr("Move %1 to “%2”? The files are copied to the drive, then deleted from the internal disk. Close KStars on both desktops first.")
                        .arg(view.size(view.usedBytes)).arg(view.chosen.label) + view.mergeNote(view.chosen)
                      : view.confirming === "back"
                      ? qsTr("Copy %1 back to the internal disk and stop using the drive? The files also stay on the drive.").arg(view.size(view.usedBytes))
                      : qsTr("Go back to an empty folder on the internal disk? Your files stay on “%1”, in astroarch/astro-data.").arg(view.driveName)
            }
            RowLayout {
                spacing: 8
                CardButton {
                    highlighted: true
                    text: view.confirming === "move" ? qsTr("Move") : view.confirming === "back" ? qsTr("Move back") : qsTr("Stop using it")
                    onClicked: view.run(view.confirming, view.confirming === "move" ? view.chosen.device : "")
                }
                CardButton {
                    text: qsTr("Cancel")
                    onClicked: view.confirming = ""
                }
            }
        }
    }
}

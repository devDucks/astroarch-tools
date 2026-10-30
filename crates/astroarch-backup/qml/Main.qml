import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import AstroArchBackup

ApplicationWindow {
    id: mainWindow
    visible: true
    width: 920
    height: 720
    minimumWidth: 720
    minimumHeight: 560
    title: qsTr("AstroArch Backup Assistant")
    color: "#1b2838"

    // ---------------------------------------------------------------- status
    // exclusions -> measuring -> location -> (media) -> ready -> running
    property string currentState: "exclusions"
    // Dossiers exclus : chemins relatifs au dossier personnel
    property var excludedPaths: []
    property real dataBytes: 0
    property var systemEval: ({ "enough": false, "problem": "", "needed": 0, "available": 0, "existing": 0 })
    property string destinationPath: ""
    property string destinationKind: ""   // "system" ou "media"
    property string destinationName: ""
    property var mediaList: []
    property string mediaJson: ""
    property bool mounting: false
    property int keepSnapshots: 5
    property string homePath: ""
    // “/srv/astro-data” if this shared folder exists, otherwise “”
    property string sharedPath: ""

    // --- complete restoration
    property string restoreDest: ""
    property string restoreDestName: ""
    property bool systemHasBackup: false
    property var snapshotList: []
    property bool loadingSnapshots: false
    property string restoreSnapshot: ""
    property bool safetyBackup: true
    property bool restoreAfterBackup: false
    // Storage media that may contain a backup (excluding FAT and unusable disks)
    property var restoreMedia: mediaList.filter(function (m) {
        return m.state !== "fat32" && m.state !== "unsupported"
    })

    // Required destination on the primary disk (FHS)
    readonly property string systemDir: "/var/backups"

    // ------------------------------------------------------------- functions
    function formatBytes(n) {
        var units = ["B", "KiB", "MiB", "GiB", "TiB"]
        var v = Number(n)
        var i = 0
        while (v >= 1024 && i < units.length - 1) {
            v /= 1024
            i++
        }
        return v.toFixed(i === 0 ? 0 : 2) + " " + units[i]
    }

    function stageLabel(code) {
        switch (code) {
        case "preparing": return qsTr("Preparing the backup file...")
        case "copying": return qsTr("Copying files...")
        case "boot": return qsTr("Copying the boot partition...")
        case "snapshot": return qsTr("Creating the snapshot...")
        case "verifying": return qsTr("Verifying the backup...")
        case "pruning": return qsTr("Removing old backups...")
        case "shrinking": return qsTr("Reducing the backup file...")
        case "finishing": return qsTr("Finishing...")
        default: return ""
        }
    }

    function stepSubtitle() {
        switch (currentState) {
        case "exclusions": return qsTr("Step 1: Choose the folders to exclude")
        case "measuring": return qsTr("Analysis in progress...")
        case "location": return qsTr("Step 2: Where to save the backup")
        case "media": return qsTr("Step 3: Choose an external drive")
        case "ready": return qsTr("Step 4: Ready to back up")
        case "restore_source": return qsTr("Restore: where is the backup?")
        case "restore_snapshots": return qsTr("Restore: choose the backup")
        case "restore_confirm": return qsTr("Restore: confirm")
        case "restore_starting": return qsTr("Starting the restore...")
        case "running": return qsTr("Backup in progress...")
        default: return ""
        }
    }

    function startMeasure() {
        currentState = "measuring"
        backupManager.measure_data(excludedPaths.join("\n"))
    }

    function handleMeasured(bytes, errorText) {
        if (errorText !== "") {
            messageDialog.show(qsTr("❌ Error"), qsTr("The space analysis failed:\n\n%1").arg(errorText))
            currentState = "exclusions"
            return
        }
        dataBytes = bytes
        if (restoreAfterBackup) {
            // Save a backup of the current state to the same location as the restore
            var ev = JSON.parse(backupManager.evaluate_destination(restoreDest))
            if (!ev.enough) {
                restoreAfterBackup = false
                messageDialog.show(qsTr("⚠️  Not enough space"),
                    qsTr("There is not enough space for the safety backup (%1 needed, %2 available).\n\nFree some space, or untick the safety backup.")
                        .arg(formatBytes(ev.needed)).arg(formatBytes(ev.available)))
                currentState = "restore_confirm"
                return
            }
            currentState = "running"
            backupManager.start_backup(restoreDest, excludedPaths.join("\n"), 100, false)
            return
        }
        systemEval = JSON.parse(backupManager.evaluate_destination(systemDir))
        currentState = "location"
    }

    // ---- complete restoration
    function openRestore() {
        mediaJson = ""
        refreshMedia()
        systemHasBackup = backupManager.has_backup(systemDir)
        currentState = "restore_source"
    }

    function chooseRestoreSource(path, name) {
        restoreDest = path
        restoreDestName = name
        snapshotList = []
        loadingSnapshots = true
        currentState = "restore_snapshots"
        backupManager.list_snapshots(path)
    }

    function formatSnapshot(name) {
        // 2026-10-16_065058 -> 2026-10-16  06:50:58
        if (name.length !== 17)
            return name
        return name.substr(0, 10) + "   " + name.substr(11, 2) + ":" + name.substr(13, 2) + ":" + name.substr(15, 2)
    }

    // A snapshot of another architecture (a Raspberry Pi backup on a PC...): its system cannot
    // run here, its settings and data can be restored by the Control Center
    function archName(arch) {
        return ({ aarch64: qsTr("Raspberry Pi"), x86_64: qsTr("PC"), arm: qsTr("32-bit ARM") })[arch] || arch
    }
    function foreignText(arch) {
        return qsTr("From a %1 system (%2): settings and data only").arg(archName(arch)).arg(arch)
    }
    // Not restorable whole: another architecture, or one that cannot be recognized
    function notRestorableText(snap) {
        return snap.foreign ? foreignText(snap.arch)
                            : qsTr("The system of this backup cannot be recognized: settings and data only")
    }
    readonly property string controlCenterText: qsTr("The whole system cannot be restored here. Its KStars, INDI and PHD2 settings and its images can: in the AstroArch Control Center, page “Restore from a backup”, source “A system backup”.")

    function chooseSnapshot(name) {
        restoreSnapshot = name
        safetyBackup = true
        confirmField.text = ""
        currentState = "restore_confirm"
    }

    function beginRestore() {
        if (safetyBackup) {
            restoreAfterBackup = true
            currentState = "measuring"
            backupManager.measure_data(excludedPaths.join("\n"))
        } else {
            startRestoreNow()
        }
    }

    function startRestoreNow() {
        currentState = "restore_starting"
        backupManager.start_restore(restoreDest, restoreSnapshot)
    }

    function chooseSystemDisk() {
        destinationPath = systemDir
        destinationKind = "system"
        destinationName = ""
        currentState = "ready"
    }

    function chooseExternalDrive() {
        mediaJson = ""
        refreshMedia()
        currentState = "media"
    }

    function refreshMedia() {
        var json = backupManager.list_media()
        if (json !== mediaJson) {
            mediaJson = json
            mediaList = JSON.parse(json)
        }
    }

    function mediaDetails(m) {
        var text = m.fstype + "  ·  " + formatBytes(m.size)
        if (m.mountpoint !== "")
            text += "  ·  " + qsTr("%1 available").arg(formatBytes(m.available))
        else
            text += "  ·  " + qsTr("Not mounted")
        return text
    }

    function mediaStatus(m) {
        switch (m.state) {
        case "ready": return qsTr("✅ Enough space")
        case "too_small": return qsTr("❌ Not enough space (%1 needed)").arg(formatBytes(m.needed))
        case "fat32": return qsTr("FAT32: cannot be used")
        case "readonly": return qsTr("Read-only")
        case "unsupported": return qsTr("Cannot be used")
        default: return ""
        }
    }

    function mediaStatusColor(m) {
        switch (m.state) {
        case "ready": return "#27ae60"
        case "too_small": return "#e74c3c"
        case "not_mounted": return "#7f8c8d"
        default: return "#e67e22"
        }
    }

    function selectMedia(m) {
        if (m.state === "ready")
            mediaConfirmDialog.openFor(m)
        else if (m.state === "too_small")
            tooSmallDialog.openFor(m)
    }

    function startBackup() {
        currentState = "running"
        backupManager.start_backup(destinationPath, excludedPaths.join("\n"), keepSnapshots, true)
    }

    // You can exit at any time: any save in progress will be canceled
    // (the helper will finish cleaning up on its own)
    function requestQuit() {
        if (backupManager.isRunning)
            confirmQuitDialog.open()
        else
            Qt.quit()
    }

    onClosing: (close) => {
        folderTree.visible = false
        if (backupManager.isRunning) {
            close.accepted = false
            confirmQuitDialog.open()
        }
    }

    Component.onCompleted: {
        homePath = backupManager.home_path()
        sharedPath = backupManager.shared_data_path()
        backupManager.load_config()
    }

    Timer {
        // Media detection: the list updates automatically
        interval: 3000
        repeat: true
        running: (mainWindow.currentState === "media" || mainWindow.currentState === "restore_source")
                 && !mainWindow.mounting
        onTriggered: mainWindow.refreshMedia()
    }

    // --------------------------------------------------------------- engine
    BackupManager {
        id: backupManager

        onConfigLoaded: (keep, excluded) => {
            mainWindow.keepSnapshots = keep
            // Most recently excluded files: resubmitted
            mainWindow.excludedPaths = excluded === "" ? [] : excluded.split("\n")
        }

        onSnapshotsListed: (json, errorText) => {
            mainWindow.loadingSnapshots = false
            if (errorText !== "") {
                messageDialog.show(qsTr("❌ Error"), errorText)
                mainWindow.currentState = "restore_source"
                return
            }
            mainWindow.snapshotList = JSON.parse(json)
        }

        onRestoreStarted: () => {
            // The desktop will close: the rest of the process will take place in text mode
        }

        onRestoreFailed: (code, errorText) => {
            // 44: a backup of another architecture (the helper checks it too)
            if (code === 44) {
                const snap = mainWindow.snapshotList.find(s => s.name === mainWindow.restoreSnapshot)
                messageDialog.show(qsTr("❌ Error"),
                    (snap ? mainWindow.notRestorableText(snap) + "\n\n" : "")
                    + mainWindow.controlCenterText + "\n\n" + qsTr("Nothing was changed."))
                mainWindow.currentState = "restore_snapshots"
                return
            }
            messageDialog.show(qsTr("❌ Error"),
                qsTr("The restore could not start. Nothing was changed.\n\n%1").arg(errorText))
            mainWindow.currentState = "restore_confirm"
        }

        onDataMeasured: (bytes, errorText) => {
            mainWindow.handleMeasured(bytes, errorText)
        }

        onMediaMounted: (device, mountpoint, errorText) => {
            mainWindow.mounting = false
            if (errorText !== "")
                messageDialog.show(qsTr("❌ Error"), errorText)
            mainWindow.refreshMedia()
        }

        onBackupFinished: (success, errorMessage) => {
            if (success && mainWindow.restoreAfterBackup) {
                // Backup complete: moving on to the restore
                mainWindow.restoreAfterBackup = false
                mainWindow.startRestoreNow()
                return
            }
            if (!success && mainWindow.restoreAfterBackup) {
                mainWindow.restoreAfterBackup = false
                messageDialog.show(qsTr("❌ Error"),
                    qsTr("Error while saving") + (errorMessage !== "" ? "\n\n" + errorMessage : ""))
                mainWindow.currentState = "restore_confirm"
                return
            }
            if (success) {
                messageDialog.show(qsTr("✅ Success"), qsTr("Backup completed successfully!"))
                mainWindow.currentState = "exclusions"
            } else {
                messageDialog.show(qsTr("❌ Error"),
                    qsTr("Error while saving") + (errorMessage !== "" ? "\n\n" + errorMessage : ""))
                // Failure: Return to step 4 to try again immediately
                mainWindow.currentState = "ready"
            }
        }

        onBackupCanceled: () => {
            if (mainWindow.restoreAfterBackup) {
                mainWindow.restoreAfterBackup = false
                mainWindow.currentState = "restore_confirm"
            } else {
                mainWindow.currentState = "ready"
            }
        }
    }

    // External folder selection window
    FolderTreeWindow {
        id: folderTree
        backend: backupManager
        transientParent: mainWindow
        onSelectionAccepted: (paths) => {
            mainWindow.excludedPaths = paths
        }
    }

    // ------------------------------------------------------------ interface
    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // En-tête
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 84
            color: "#243447"

            ColumnLayout {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: 20
                spacing: 4

                Text {
                    text: qsTr("🔷 AstroArch Backup Assistant")
                    color: "white"
                    font.pixelSize: 22
                    font.bold: true
                }

                Text {
                    text: mainWindow.stepSubtitle()
                    color: "#9fb3c8"
                    font.pixelSize: 12
                }
            }
        }

        // Contenu
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            Rectangle {
                anchors.fill: parent
                anchors.margins: 20
                color: "#ecf0f1"
                radius: 8

                // ------------------------------------------ 1. exclusions
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 14
                    visible: mainWindow.currentState === "exclusions"

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Step 1: Choose the folders to exclude")
                        font.pixelSize: 18
                        font.bold: true
                        color: "#2c3e50"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: mainWindow.sharedPath === ""
                              ? qsTr("The whole system is backed up. Choose the folders of %1 that you do NOT want to include: checked folders and everything inside them are left out.").arg(mainWindow.homePath)
                              : qsTr("The whole system is backed up. Choose the folders that you do NOT want to include, in your home folder %1 or in the shared data folder %2: checked folders and everything inside them are left out.").arg(mainWindow.homePath).arg(mainWindow.sharedPath)
                        color: "#34495e"
                        wrapMode: Text.Wrap
                    }

                    Button {
                        text: qsTr("📂 Choose the folders...")
                        onClicked: folderTree.openWith(mainWindow.excludedPaths)
                    }

                    Text {
                        text: qsTr("Excluded folders:")
                        font.bold: true
                        color: "#2c3e50"
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        color: "white"
                        border.color: "#bdc3c7"
                        radius: 4

                        ScrollView {
                            id: excludedScroll
                            anchors.fill: parent
                            anchors.margins: 8
                            clip: true

                            Text {
                                width: excludedScroll.availableWidth
                                text: mainWindow.excludedPaths.length === 0
                                      ? qsTr("No folders excluded")
                                      : mainWindow.excludedPaths.join("\n")
                                color: "#2c3e50"
                                wrapMode: Text.Wrap
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true

                        Button {
                            text: qsTr("♻️  Restore a backup...")
                            onClicked: mainWindow.openRestore()
                        }

                        Item {
                            Layout.fillWidth: true
                        }

                        Button {
                            text: qsTr("Next →")
                            onClicked: mainWindow.startMeasure()
                        }
                    }
                }

                // ------------------------------------------ analysis in progress
                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 16
                    visible: mainWindow.currentState === "measuring"

                    BusyIndicator {
                        Layout.alignment: Qt.AlignHCenter
                        running: mainWindow.currentState === "measuring"
                    }

                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: qsTr("Analysis in progress...")
                        font.pixelSize: 16
                        color: "#2c3e50"
                    }

                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: qsTr("A password may be requested to read all the system files.")
                        color: "#7f8c8d"
                    }
                }

                // ------------------------------------------ 2. location
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 14
                    visible: mainWindow.currentState === "location"

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Step 2: Where to save the backup")
                        font.pixelSize: 18
                        font.bold: true
                        color: "#2c3e50"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Data to back up: %1").arg(mainWindow.formatBytes(mainWindow.dataBytes))
                        color: "#34495e"
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: mainWindow.systemEval.enough
                        text: qsTr("There is enough space on the main disk, in %1 (space needed: %2, available: %3).\n\nThis location is fixed and cannot be changed.")
                              .arg(mainWindow.systemDir)
                              .arg(mainWindow.formatBytes(mainWindow.systemEval.needed))
                              .arg(mainWindow.formatBytes(mainWindow.systemEval.available))
                        color: "#27ae60"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: !mainWindow.systemEval.enough && mainWindow.systemEval.problem === ""
                        text: qsTr("There is not enough space on the main disk, in %1 (space needed: %2, available: %3).\n\nYou must save the backup on an external drive.")
                              .arg(mainWindow.systemDir)
                              .arg(mainWindow.formatBytes(mainWindow.systemEval.needed))
                              .arg(mainWindow.formatBytes(mainWindow.systemEval.available))
                        color: "#e74c3c"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: !mainWindow.systemEval.enough && mainWindow.systemEval.problem !== ""
                        text: qsTr("The location %1 on the main disk cannot be used (%2).\n\nYou must save the backup on an external drive.")
                              .arg(mainWindow.systemDir)
                              .arg(mainWindow.systemEval.problem)
                        color: "#e74c3c"
                        wrapMode: Text.Wrap
                    }

                    Item {
                        Layout.fillHeight: true
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Button {
                            text: qsTr("← Back")
                            onClicked: mainWindow.currentState = "exclusions"
                        }

                        Item {
                            Layout.fillWidth: true
                        }

                        Button {
                            visible: mainWindow.systemEval.enough
                            text: qsTr("Save on the main disk")
                            onClicked: mainWindow.chooseSystemDisk()
                        }

                        Button {
                            text: qsTr("Use an external drive")
                            onClicked: mainWindow.chooseExternalDrive()
                        }
                    }
                }

                // ------------------------------------------ 3. external media
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 12
                    visible: mainWindow.currentState === "media"

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Step 3: Choose an external drive")
                        font.pixelSize: 18
                        font.bold: true
                        color: "#2c3e50"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("The backup will be created in the folder astroarch/backups of the drive. The list refreshes automatically when a drive is plugged in.")
                        color: "#34495e"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("⚠️  Drives formatted in FAT32 cannot be used (4 GB file limit).")
                        font.pixelSize: 11
                        color: "#e67e22"
                        wrapMode: Text.Wrap
                    }

                    ListView {
                        id: mediaView
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        spacing: 8
                        model: mainWindow.mediaList

                        delegate: Rectangle {
                            width: mediaView.width
                            height: 72
                            radius: 6
                            color: "white"
                            border.color: "#bdc3c7"

                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 10
                                spacing: 12

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        Layout.fillWidth: true
                                        text: modelData.label + "  (" + modelData.device + ")"
                                        font.bold: true
                                        color: "#2c3e50"
                                        elide: Text.ElideRight
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: mainWindow.mediaDetails(modelData)
                                        color: "#7f8c8d"
                                        font.pixelSize: 11
                                        elide: Text.ElideRight
                                    }
                                }

                                Text {
                                    text: mainWindow.mediaStatus(modelData)
                                    color: mainWindow.mediaStatusColor(modelData)
                                    font.pixelSize: 12
                                }

                                Button {
                                    visible: modelData.state === "ready" || modelData.state === "too_small"
                                    text: qsTr("Select")
                                    onClicked: mainWindow.selectMedia(modelData)
                                }

                                Button {
                                    visible: modelData.state === "not_mounted"
                                    enabled: !mainWindow.mounting
                                    text: qsTr("Mount")
                                    onClicked: {
                                        mainWindow.mounting = true
                                        backupManager.mount_media(modelData.device)
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: mainWindow.mediaList.length === 0
                        text: qsTr("No external drive found. Plug in a drive.")
                        color: "#7f8c8d"
                        wrapMode: Text.Wrap
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Button {
                            text: qsTr("← Back")
                            onClicked: mainWindow.currentState = "location"
                        }

                        Item {
                            Layout.fillWidth: true
                        }

                        Button {
                            text: qsTr("↻ Refresh")
                            onClicked: mainWindow.refreshMedia()
                        }
                    }
                }

                // ------------------------------------------ 4. ready
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 14
                    visible: mainWindow.currentState === "ready"

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Step 4: Ready to back up")
                        font.pixelSize: 18
                        font.bold: true
                        color: "#2c3e50"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Destination: %1").arg(mainWindow.destinationPath)
                        color: "#34495e"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: mainWindow.destinationKind === "system"
                              ? qsTr("On the main disk (fixed location)")
                              : qsTr("On the external drive %1").arg(mainWindow.destinationName)
                        color: "#7f8c8d"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Data to back up: %1").arg(mainWindow.formatBytes(mainWindow.dataBytes))
                        color: "#34495e"
                    }

                    RowLayout {
                        spacing: 12

                        Text {
                            text: qsTr("Number of backups to keep:")
                            color: "#2c3e50"
                            font.bold: true
                        }

                        SpinBox {
                            from: 1
                            to: 50
                            editable: true
                            value: mainWindow.keepSnapshots
                            onValueModified: mainWindow.keepSnapshots = value
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("When this number is exceeded, the oldest backups are deleted automatically. Keeping more backups uses more space.")
                        font.pixelSize: 11
                        color: "#7f8c8d"
                        wrapMode: Text.Wrap
                    }

                    Item {
                        Layout.fillHeight: true
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Button {
                            text: qsTr("← Back")
                            onClicked: mainWindow.currentState = (mainWindow.destinationKind === "media" ? "media" : "location")
                        }

                        Item {
                            Layout.fillWidth: true
                        }

                        Button {
                            text: qsTr("▶️  START THE BACKUP")
                            onClicked: mainWindow.startBackup()
                        }
                    }
                }

                // ------------------------------------------ restoration: location
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 12
                    visible: mainWindow.currentState === "restore_source"

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Restore: where is the backup?")
                        font.pixelSize: 18
                        font.bold: true
                        color: "#2c3e50"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Choose the location that contains the backups, on the main disk or on an external drive. A restore replaces the WHOLE system by the state of a backup.")
                        color: "#34495e"
                        wrapMode: Text.Wrap
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 64
                        visible: mainWindow.systemHasBackup
                        radius: 6
                        color: "white"
                        border.color: "#bdc3c7"

                        RowLayout {
                            anchors.fill: parent
                            anchors.margins: 10
                            spacing: 12

                            Text {
                                Layout.fillWidth: true
                                text: qsTr("Main disk (%1)").arg(mainWindow.systemDir)
                                font.bold: true
                                color: "#2c3e50"
                            }

                            Button {
                                text: qsTr("Select")
                                onClicked: mainWindow.chooseRestoreSource(mainWindow.systemDir,
                                                                          qsTr("Main disk (%1)").arg(mainWindow.systemDir))
                            }
                        }
                    }

                    ListView {
                        id: restoreMediaView
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        spacing: 8
                        model: mainWindow.restoreMedia

                        delegate: Rectangle {
                            width: restoreMediaView.width
                            height: 72
                            radius: 6
                            color: "white"
                            border.color: "#bdc3c7"

                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 10
                                spacing: 12

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        Layout.fillWidth: true
                                        text: modelData.label + "  (" + modelData.device + ")"
                                        font.bold: true
                                        color: "#2c3e50"
                                        elide: Text.ElideRight
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: mainWindow.mediaDetails(modelData)
                                        color: "#7f8c8d"
                                        font.pixelSize: 11
                                        elide: Text.ElideRight
                                    }
                                }

                                Text {
                                    visible: modelData.state !== "not_mounted" && !modelData.hasBackup
                                    text: qsTr("No backup on this drive")
                                    color: "#7f8c8d"
                                    font.pixelSize: 12
                                }

                                Button {
                                    visible: modelData.hasBackup === true
                                    text: qsTr("Select")
                                    onClicked: mainWindow.chooseRestoreSource(modelData.destination, modelData.label)
                                }

                                Button {
                                    visible: modelData.state === "not_mounted"
                                    enabled: !mainWindow.mounting
                                    text: qsTr("Mount")
                                    onClicked: {
                                        mainWindow.mounting = true
                                        backupManager.mount_media(modelData.device)
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: !mainWindow.systemHasBackup && mainWindow.restoreMedia.length === 0
                        text: qsTr("No backup found. Plug in the drive that contains your backups.")
                        color: "#7f8c8d"
                        wrapMode: Text.Wrap
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Button {
                            text: qsTr("← Back")
                            onClicked: mainWindow.currentState = "exclusions"
                        }

                        Item {
                            Layout.fillWidth: true
                        }

                        Button {
                            text: qsTr("↻ Refresh")
                            onClicked: mainWindow.refreshMedia()
                        }
                    }
                }

                // ------------------------------------------ restoration: Selecting a Snapshot
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 12
                    visible: mainWindow.currentState === "restore_snapshots"

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Restore: choose the backup")
                        font.pixelSize: 18
                        font.bold: true
                        color: "#2c3e50"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: mainWindow.restoreDestName
                        color: "#7f8c8d"
                        elide: Text.ElideMiddle
                    }

                    RowLayout {
                        visible: mainWindow.loadingSnapshots
                        spacing: 12

                        BusyIndicator {
                            running: mainWindow.loadingSnapshots
                        }

                        Text {
                            text: qsTr("Reading the backups... A password may be requested.")
                            color: "#34495e"
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: !mainWindow.loadingSnapshots && mainWindow.snapshotList.length === 0
                        text: qsTr("No backup found in this location.")
                        color: "#7f8c8d"
                    }

                    ListView {
                        id: snapshotView
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        spacing: 8
                        model: mainWindow.snapshotList

                        delegate: Rectangle {
                            width: snapshotView.width
                            height: Math.max(56, snapshotRow.implicitHeight + 20)
                            radius: 6
                            color: "white"
                            border.color: "#bdc3c7"

                            RowLayout {
                                id: snapshotRow
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.margins: 10
                                spacing: 12

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        Layout.fillWidth: true
                                        text: mainWindow.formatSnapshot(modelData.name)
                                        font.bold: true
                                        color: "#2c3e50"
                                    }

                                    // Another or an unknown architecture: the Control Center restores its
                                    // settings and data
                                    Text {
                                        Layout.fillWidth: true
                                        visible: modelData.restorable !== true
                                        text: mainWindow.notRestorableText(modelData) + "\n" + mainWindow.controlCenterText
                                        color: "#e67e22"
                                        font.pixelSize: 12
                                        wrapMode: Text.Wrap
                                    }
                                }

                                Text {
                                    visible: !modelData.boot && modelData.restorable === true
                                    text: qsTr("No boot partition: cannot be restored")
                                    color: "#e67e22"
                                    font.pixelSize: 12
                                }

                                Button {
                                    visible: modelData.boot === true && modelData.restorable === true
                                    text: qsTr("Restore this backup")
                                    onClicked: mainWindow.chooseSnapshot(modelData.name)
                                }
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true

                        Button {
                            text: qsTr("← Back")
                            onClicked: mainWindow.currentState = "restore_source"
                        }

                        Item {
                            Layout.fillWidth: true
                        }
                    }
                }

                // ------------------------------------------ restoration: confirmation
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 10
                    visible: mainWindow.currentState === "restore_confirm"

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Restore: confirm")
                        font.pixelSize: 18
                        font.bold: true
                        color: "#c0392b"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Backup to restore: %1").arg(mainWindow.formatSnapshot(mainWindow.restoreSnapshot))
                        font.bold: true
                        color: "#2c3e50"
                        wrapMode: Text.Wrap
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        color: "#fdecea"
                        border.color: "#e74c3c"
                        radius: 4

                        ScrollView {
                            id: warningScroll
                            anchors.fill: parent
                            anchors.margins: 10
                            clip: true

                            Text {
                                width: warningScroll.availableWidth
                                text: qsTr("⚠️  FULL SYSTEM RESTORE\n\nThe whole system will be replaced by the state of this backup.\n\n• Everything that changed since this backup is permanently lost: files, installed programs and updates, settings.\n• The contents of the folders you excluded from the backups are deleted (the folders themselves are kept, empty).\n• The desktop closes and the restore runs in text mode; it can take several minutes. The Raspberry Pi restarts by itself when it is finished.\n• Do NOT switch off or unplug the Raspberry Pi during the restore: a power cut or a manual stop can leave the system unusable. This risk is yours.\n• It only works on the same disk: a backup made for another disk is refused.")
                                color: "#7b241c"
                                wrapMode: Text.Wrap
                                font.pixelSize: 12
                            }
                        }
                    }

                    CheckBox {
                        text: qsTr("First make a safety backup of the current state (recommended)")

                        contentItem: Text {
                            text: parent.text
                            font: parent.font
                            color: "#7f8c8d"
                            leftPadding: parent.indicator.width + parent.spacing
                            verticalAlignment: Text.AlignVCenter
                        }

                        checked: mainWindow.safetyBackup
                        onToggled: mainWindow.safetyBackup = checked
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("The safety backup lets you undo the restore later by restoring it.")
                        font.pixelSize: 11
                        color: "#7f8c8d"
                        wrapMode: Text.Wrap
                    }

                    RowLayout {
                        spacing: 10

                        Text {
                            text: qsTr("Type %1 to confirm:").arg("RESTORE")
                            color: "#2c3e50"
                            font.bold: true
                        }

                        TextField {
                            id: confirmField
                            Layout.preferredWidth: 160
                            placeholderText: "RESTORE"
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Button {
                            text: qsTr("← Back")
                            onClicked: mainWindow.currentState = "restore_snapshots"
                        }

                        Item {
                            Layout.fillWidth: true
                        }

                        Button {
                            text: qsTr("Restore the system")
                            enabled: confirmField.text === "RESTORE"
                            onClicked: mainWindow.beginRestore()
                        }
                    }
                }

                // ------------------------------------------ restoration: start
                ColumnLayout {
                    anchors.centerIn: parent
                    width: parent.width - 80
                    spacing: 16
                    visible: mainWindow.currentState === "restore_starting"

                    BusyIndicator {
                        Layout.alignment: Qt.AlignHCenter
                        running: mainWindow.currentState === "restore_starting"
                    }

                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: qsTr("Starting the restore...")
                        font.pixelSize: 16
                        font.bold: true
                        color: "#2c3e50"
                    }

                    Text {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        text: qsTr("The desktop will close in a few seconds and the restore will continue in text mode. Do NOT switch off or unplug the Raspberry Pi: it restarts by itself when it is finished.")
                        color: "#c0392b"
                        wrapMode: Text.Wrap
                    }
                }

                // ------------------------------------------ in progress
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 14
                    visible: mainWindow.currentState === "running"

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Backup in progress...")
                        font.pixelSize: 18
                        font.bold: true
                        color: "#2c3e50"
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: mainWindow.restoreAfterBackup
                        text: qsTr("Safety backup before the restore...")
                        font.bold: true
                        color: "#e67e22"
                        wrapMode: Text.Wrap
                    }

                    Text {
                        text: qsTr("Progress:")
                        color: "#34495e"
                    }

                    ProgressBar {
                        id: backupProgress
                        Layout.fillWidth: true
                        from: 0
                        to: 1
                        value: backupManager.progressPercent / 100.0

                        // Explicit delegates: we do not use those in the Breeze style
                        background: Rectangle {
                            implicitHeight: 10
                            radius: 5
                            color: "#2c3e50"
                        }
                        contentItem: Item {
                            implicitHeight: 10
                            Rectangle {
                                width: backupProgress.visualPosition * parent.width
                                height: parent.height
                                radius: 5
                                color: "#2ecc71"
                            }
                        }
                    }

                    Text {
                        text: backupManager.progressPercent + "%"
                        font.pixelSize: 24
                        font.bold: true
                        color: "#2c3e50"
                    }

                    Text {
                        Layout.fillWidth: true
                        text: mainWindow.stageLabel(backupManager.currentFile)
                        color: "#7f8c8d"
                        wrapMode: Text.Wrap
                    }

                    Item {
                        Layout.fillHeight: true
                    }
                }
            }
        }

        // Footer: “Quit” is ALWAYS available
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 64
            color: "#243447"

            RowLayout {
                anchors.fill: parent
                anchors.margins: 12
                spacing: 10

                Button {
                    text: qsTr("Exit")
                    onClicked: mainWindow.requestQuit()
                }

                Item {
                    Layout.fillWidth: true
                }

                Button {
                    visible: backupManager.isRunning
                    text: qsTr("⚠️  Cancel the backup")
                    onClicked: cancelBackupDialog.open()
                }
            }
        }
    }

    // ------------------------------------------------------------- dialogues
    Dialog {
        id: messageDialog
        property string message: ""
        anchors.centerIn: Overlay.overlay
        modal: true
        width: 540
        standardButtons: Dialog.Ok

        function show(dialogTitle, text) {
            title = dialogTitle
            message = text
            open()
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16

            Text {
                Layout.fillWidth: true
                text: messageDialog.message
                color: palette.windowText
                wrapMode: Text.Wrap
                font.pixelSize: 12
            }
        }
    }

    Dialog {
        id: mediaConfirmDialog
        property var media: ({})
        anchors.centerIn: Overlay.overlay
        modal: true
        width: 540
        title: qsTr("Save on this drive?")

        function openFor(m) {
            media = m
            open()
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 16

            Text {
                Layout.fillWidth: true
                text: qsTr("Create the backup in the folder\n%1\n\non the drive %2?")
                      .arg(mediaConfirmDialog.media.destination !== undefined ? mediaConfirmDialog.media.destination : "")
                      .arg(mediaConfirmDialog.media.label !== undefined ? mediaConfirmDialog.media.label : "")
                color: palette.windowText
                wrapMode: Text.Wrap
                font.pixelSize: 12
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 10

                Button {
                    text: qsTr("No, choose another drive")
                    onClicked: mediaConfirmDialog.close()
                }

                Button {
                    text: qsTr("Yes, save here")
                    onClicked: {
                        mainWindow.destinationPath = mediaConfirmDialog.media.destination
                        mainWindow.destinationKind = "media"
                        mainWindow.destinationName = mediaConfirmDialog.media.label
                        mainWindow.currentState = "ready"
                        mediaConfirmDialog.close()
                    }
                }
            }
        }
    }

    Dialog {
        id: tooSmallDialog
        property var media: ({})
        anchors.centerIn: Overlay.overlay
        modal: true
        width: 540
        title: qsTr("⚠️  Not enough space")
        standardButtons: Dialog.Ok

        function openFor(m) {
            media = m
            open()
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16

            Text {
                Layout.fillWidth: true
                text: qsTr("This drive does not have enough space for the backup (%1 needed, %2 available).\n\nPlease choose another drive.")
                      .arg(mainWindow.formatBytes(tooSmallDialog.media.needed !== undefined ? tooSmallDialog.media.needed : 0))
                      .arg(mainWindow.formatBytes(tooSmallDialog.media.available !== undefined ? tooSmallDialog.media.available : 0))
                color: palette.windowText
                wrapMode: Text.Wrap
                font.pixelSize: 12
            }
        }
    }

    Dialog {
        id: cancelBackupDialog
        anchors.centerIn: Overlay.overlay
        modal: true
        width: 560
        title: qsTr("Cancel the backup?")

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 16

            Text {
                Layout.fillWidth: true
                text: qsTr("Are you sure you want to cancel the backup?\n\nThe backup in progress will be stopped.\nYour previous backups are NOT affected.\nIf this is your first backup, the unfinished backup file will be removed.")
                color: palette.windowText
                wrapMode: Text.Wrap
                font.pixelSize: 12
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 10

                Button {
                    text: qsTr("No, continue the backup")
                    onClicked: cancelBackupDialog.close()
                }

                Button {
                    text: qsTr("Yes, cancel and clean up")
                    onClicked: {
                        backupManager.cancel_backup()
                        cancelBackupDialog.close()
                    }
                }
            }
        }
    }

    Dialog {
        id: confirmQuitDialog
        anchors.centerIn: Overlay.overlay
        modal: true
        width: 560
        title: qsTr("⚠️  ATTENTION")

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 16

            Text {
                Layout.fillWidth: true
                text: qsTr("A backup is running.\n\nQuitting will cancel it. Your previous backups are NOT affected.\n\nQuit anyway?")
                color: palette.windowText
                wrapMode: Text.Wrap
                font.pixelSize: 12
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 10

                Button {
                    text: qsTr("No, continue")
                    onClicked: confirmQuitDialog.close()
                }

                Button {
                    text: qsTr("Yes, leave")
                    onClicked: {
                        backupManager.cancel_backup()
                        Qt.quit()
                    }
                }
            }
        }
    }
}

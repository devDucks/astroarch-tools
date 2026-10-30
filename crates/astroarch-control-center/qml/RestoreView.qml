import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import AstroArchControlCenter

// The Restore page: KStars/Ekos, INDI and PHD2 profiles and settings (and images) brought
// back from a data backup, a system backup (a snapshot of the backup tool's image, mounted
// read-only by its BackupManager; even one of another architecture), an old AstroArch disk
// or AstroMonitor. What can be restored is
// the catalog of the data backup (the same tick list, SelectionList); the logic is
// RestoreManager (src/restore.rs), the copy its root helper. A wizard of three steps in
// the card, as the Data backup page
ColumnLayout {
    id: view

    // A restore was done: the setup step is done
    signal restored()
    signal scrollToTop()
    // Scroll the page to show this item (Main.qml)
    signal reveal(Item item)
    // Where the message and the wizard's buttons go, pinned under the page (Main.qml)
    property Item navHost: null
    readonly property bool navShown: view.mode === "wizard" || view.message !== ""
    // The wizard's Next / Restore / Finish is the primary button meanwhile (Main.qml)
    readonly property bool ownsPrimary: view.mode === "wizard"
    // The network folder form is open in step 1: its Connect and Cancel are the actions
    readonly property bool formOpen: view.mode === "wizard" && view.step === 1 && view.kind === "backup" && view.shareForm

    spacing: 14

    // ------------------------------------------------------------------ state
    property string mode: "start"          // "start" | "wizard"
    property int step: 1                   // 1 where from, 2 what, 3 restore
    property string kind: ""               // "backup" | "system" | "disk" | "astromonitor"
    property var source: null              // the spec RestoreManager takes, once chosen
    property var about: ({})               // RestoreManager.describe(source)
    property var places: []                // backup: places with backups (DataBackup.destinations)
    property var systems: ({})             // backup: place path -> RestoreManager.systems()
    property var chosenSystem: null        // backup: {id, folder, snapshots, ...}
    property string snapshot: ""
    property var disks: []
    // system: the places with a system backup (as the Data backup page's System profile
    // finds them), the one opened (mounted read-only) and its snapshots
    property var sysPlaces: []
    property string sysDest: ""
    property bool sysLoading: false
    property var sysSnapshots: []
    property bool shareForm: false
    property var groups: []
    property var excluded: ({})
    property var sizes: ({})
    property string running: ""            // programs to close first
    property var result: null             // the last restore: {aside, skipped, entries}
    property var asides: []
    property string message: ""
    property bool warning: false

    readonly property var steps: [qsTr("Where from"), qsTr("What to restore"), qsTr("Restore")]
    onStepChanged: view.scrollToTop()
    onModeChanged: view.scrollToTop()

    function say(text, isWarning) {
        view.message = text
        view.warning = !!isWarning
        if (text !== "")
            revealTimer.restart()
    }
    function fmt(bytes) {
        return Qt.locale().formattedDataSize(bytes)
    }
    function when(seconds) {
        return Qt.locale().toString(new Date(seconds * 1000), Locale.ShortFormat)
    }
    // snapshot-20261029-220554 (.0001), or 2026-10-29_220554 (system backup) -> its date
    function snapshotText(name) {
        if (name === "current")
            return qsTr("Latest copy (Syncthing)")
        const m = /^snapshot-(\d{4})(\d\d)(\d\d)-(\d\d)(\d\d)(\d\d)/.exec(name)
                  || /^(\d{4})-(\d\d)-(\d\d)_(\d\d)(\d\d)(\d\d)$/.exec(name)
        return m ? Qt.locale().toString(new Date(m[1], m[2] - 1, m[3], m[4], m[5], m[6]), Locale.ShortFormat) : name
    }
    // The system a snapshot of the system backup comes from (its architecture)
    function archNote(snap) {
        const name = ({ aarch64: qsTr("Raspberry Pi"), x86_64: qsTr("PC"), arm: qsTr("32-bit ARM") })[snap.arch] || snap.arch
        if (snap.foreign)
            return qsTr("From a %1 system (%2): settings and data only").arg(name).arg(snap.arch)
        return snap.arch && snap.arch !== "unknown" ? qsTr("From a %1 system (%2), like this one").arg(name).arg(snap.arch) : ""
    }
    function appNames(list) {
        return list.split(",").filter(a => a).map(a => ({ kstars: "KStars", "phd2.bin": "PHD2", indiserver: qsTr("INDI server") })[a] || a).join(", ")
    }
    function labelText(label) {
        const parts = (label || "").split("|")
        return ({ backup: qsTr("a data backup"), system: qsTr("a system backup"), disk: qsTr("an old AstroArch disk"),
                  astromonitor: "AstroMonitor" })[parts[0]] || parts[0]
    }
    function errorText(code, detail) {
        return ({
            auth: qsTr("Authentication cancelled or refused."),
            busy: qsTr("Close %1 first: they would write over the restored files.").arg(view.appNames(detail)),
            missing: qsTr("The restore helper is not installed (%1).").arg(detail),
            notfound: qsTr("No AstroMonitor backup was found for this token."),
            network: qsTr("The AstroArch server could not be reached: %1").arg(detail),
            token: qsTr("This token is not valid: copy it again from the AstroMonitor bot."),
            empty: qsTr("Nothing to restore: tick at least one row that is in the backup."),
            system: qsTr("This is the disk AstroArch runs from."),
            unsupported: qsTr("This partition cannot hold an AstroArch system (%1).").arg(detail),
            mount: qsTr("The disk could not be opened: %1").arg(detail),
            putback: qsTr("Some files could not be put back: they stay in “Earlier restores”, to try again.")
        })[code] || qsTr("It did not work: %1").arg(detail || code)
    }

    // ------------------------------------------------------------- actions
    function reloadAsides() {
        view.asides = JSON.parse(restore.asides())
    }
    function start(kind) {
        view.kind = kind
        view.source = null
        view.about = {}
        view.chosenSystem = null
        view.snapshot = ""
        view.sysSnapshots = []
        view.result = null
        view.shareForm = false
        view.say("")
        view.mode = "wizard"
        view.step = 1
        view.refreshPlaces()
        if (kind === "astromonitor")
            tokenField.text = restore.astromonitorToken()
    }
    function leave() {
        // An old disk stays open only while the page uses it
        if (view.kind === "disk" && view.source)
            restore.unmountDisk()
        // The system backup too: while it is mounted, no system backup can be made
        view.releaseSystem()
        view.mode = "start"
        view.kind = ""
        view.source = null
        view.reloadAsides()
    }
    function refreshPlaces() {
        if (view.kind === "backup") {
            const all = JSON.parse(dataBackend.destinations(0)).filter(d => d.kind !== "syncthing")
            const found = {}
            for (const p of all)
                if (p.path && p.state !== "not_mounted")
                    found[p.path] = JSON.parse(restore.systems(p.path))
            view.systems = found
            // Places with a backup, and those to mount or connect first
            view.places = all.filter(p => (found[p.path] || []).length > 0 || ["not_mounted", "automount"].includes(p.state))
        } else if (view.kind === "disk") {
            view.disks = JSON.parse(restore.disks())
        } else if (view.kind === "system") {
            // Where the Data backup page's System profile puts it: its saved place
            // (~/.backup_dest), this disk, the drives with an astroarch/backups folder
            const media = JSON.parse(dataBackend.offeredDrives(systemBackup.list_media()))
            const list = []
            for (const path of ["/var/backups", JSON.parse(dataBackend.systemConfig()).dest || ""]) {
                if (path !== "" && !list.some(p => p.path === path) && !media.some(m => m.destination === path)
                        && systemBackup.has_backup(path))
                    list.push({ kind: path === "/var/backups" ? "internal" : "folder", path: path, label: "", state: "" })
            }
            for (const m of media) {
                if (m.hasBackup || m.state === "not_mounted")
                    list.push({ kind: "usb", path: m.destination, label: m.label, device: m.device, state: m.state })
            }
            // The one opened stays listed, even while its drive is read again
            if (view.sysDest !== "" && !list.some(p => p.path === view.sysDest))
                list.push({ kind: "folder", path: view.sysDest, label: "", state: "" })
            view.sysPlaces = list
        }
    }
    // Opens a system backup read-only (the backup tool's helper: a password) and lists
    // its snapshots
    function openSystem(path) {
        view.say("")
        view.source = null
        view.snapshot = ""
        view.sysSnapshots = []
        view.sysDest = path
        view.sysLoading = true
        systemBackup.browse(path)
    }
    function releaseSystem() {
        if (view.sysDest !== "")
            systemBackup.release()
        view.sysDest = ""
        view.sysSnapshots = []
        view.sysLoading = false
    }
    function chooseSystemSnapshot(snap) {
        view.say("")
        view.snapshot = snap.name
        view.openSource({ kind: "system", root: snap.root, name: view.snapshotText(snap.name) })
        if (!view.about.found)
            view.say(qsTr("No AstroArch home folder was found in this backup."), true)
    }
    function chooseSnapshot(system, name) {
        view.chosenSystem = system
        view.snapshot = name
        view.source = { kind: "backup", folder: system.folder, snapshot: name }
    }
    function openSource(spec) {
        view.source = spec
        view.about = JSON.parse(restore.describe(JSON.stringify(spec)))
    }
    function loadRows() {
        const r = JSON.parse(restore.rows(JSON.stringify(view.source)))
        const ex = {}
        for (const k of r.excluded)
            ex[k] = true
        view.groups = r.groups
        view.excluded = ex
        view.sizes = {}
        restore.requestSizes(JSON.stringify(view.source))
    }
    function fetch() {
        if (tokenField.text.trim() === "" || restore.busy)
            return
        view.say("")
        restore.fetchAstromonitor(tokenField.text)
    }
    function next() {
        view.say("")
        if (view.step === 1) {
            view.loadRows()
            view.running = restore.runningApps()
            view.step = 2
        } else if (view.step === 2) {
            view.result = null
            view.step = 3
            restore.restore(JSON.stringify(view.source), JSON.stringify(Object.keys(view.excluded).filter(k => view.excluded[k])))
        }
    }
    function back() {
        view.say("")
        if (view.step === 1 || restore.busy)
            view.leave()
        else
            view.step = view.step === 3 ? 2 : 1
    }

    RestoreManager {
        id: restore
        onSizeReady: (key, bytes) => {
            const s = Object.assign({}, view.sizes)
            s[key] = bytes
            view.sizes = s
        }
        onFinished: (action, code, detail) => {
            if (action === "unmount")
                return
            // The page was left during the restore: the system backup is released now
            if (action === "restore" && view.kind === "system" && !view.visible)
                Qt.callLater(view.leave)
            if (code !== "") {
                view.say(view.errorText(code, detail), true)
                // A restore cut short may have set files aside already: they are listed
                if (action === "restore")
                    view.step = 2
                if (action === "restore" || action === "putback")
                    view.reloadAsides()
                return
            }
            if (action === "mount") {
                view.openSource({ kind: "disk", root: detail, name: diskName })
                if (!view.about.found)
                    view.say(qsTr("No AstroArch home folder was found on this partition: is it the system partition of your old AstroArch?"), true)
            } else if (action === "fetch") {
                view.openSource({ kind: "astromonitor", root: detail })
                view.say(qsTr("Your AstroMonitor backup was downloaded. Continue to choose what to restore."))
            } else if (action === "restore") {
                view.result = JSON.parse(detail)
                view.reloadAsides()
                view.restored()
            } else if (action === "putback") {
                view.reloadAsides()
                view.say(qsTr("What was there before the restore is back."))
                if (view.mode === "wizard")
                    view.leave()
            }
        }
        property string diskName: ""
    }
    // The system backup: the backup tool's own BackupManager (its bridge.rs) finds, mounts
    // read-only and lists it
    BackupManager {
        id: systemBackup
        onSnapshotsListed: (json, error) => {
            view.sysLoading = false
            if (view.kind !== "system" || view.sysDest === "")
                return
            if (error !== "") {
                view.releaseSystem()
                view.say(qsTr("The system backup could not be opened: %1").arg(error), true)
                return
            }
            view.sysSnapshots = JSON.parse(json).filter(s => s.root !== "")
            if (view.sysSnapshots.length === 0)
                view.say(qsTr("This system backup holds no snapshot yet."), true)
        }
        onMediaMounted: (device, mountpoint, error) => {
            if (error !== "")
                view.say(qsTr("Could not mount the drive: %1").arg(error), true)
            view.refreshPlaces()
        }
    }
    // Another page shown: the system backup is released (during a restore, which reads it,
    // when it finishes: onFinished)
    onVisibleChanged: if (!visible && view.kind === "system" && !restore.busy) view.leave()

    // The places of the data backup, and how to connect a network folder: the Data backup
    // page's own code
    DataBackup {
        id: dataBackend
        onMounted: (device, mountpoint, error) => {
            if (error !== "")
                view.say(qsTr("Could not mount the drive: %1").arg(error), true)
            view.refreshPlaces()
        }
        onShareChanged: (ok, error, path) => {
            if (!ok) {
                view.say(shareBox.errorText(error), true)
                return
            }
            view.shareForm = false
            shareBox.clearPassword()
            view.refreshPlaces()
        }
    }

    Component.onCompleted: view.reloadAsides()

    // The message where it is shown, once laid out: under the network folder form's
    // Connect, else pinned under the page with the buttons (always seen)
    Timer {
        id: revealTimer
        interval: 300
        onTriggered: {
            const target = [shareBox.messageItem, navMessage].find(m => m.visible)
            if (target && view.visible)
                view.reveal(target)
        }
    }

    // Drives plugged in, programs closed: followed while the wizard is open
    Timer {
        interval: 3000
        repeat: true
        running: view.visible && view.mode === "wizard" && !restore.busy && !dataBackend.busy
        onTriggered: {
            if (view.step === 1 && view.kind !== "astromonitor")
                view.refreshPlaces()
            view.running = restore.runningApps()
        }
    }

    component Separator: Rectangle {
        Layout.fillWidth: true
        implicitHeight: 1
        color: "#e1e6ea"
    }

    // One choice of a list: icon, name, small lines, buttons on the right
    component ChoiceRow: Rectangle {
        id: choiceRow
        property string icon
        property string title
        property string detail
        property string note
        property color noteColor: "#5d6d7e"
        property bool chosen: false
        default property alias buttons: rowButtons.data
        Layout.fillWidth: true
        implicitHeight: rowLayout.implicitHeight + 20
        color: chosen ? "#e3f0f8" : "transparent"
        border.width: chosen ? 2 : 0
        border.color: "#1d6fa5"
        radius: 6
        RowLayout {
            id: rowLayout
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: 14
            anchors.rightMargin: 14
            spacing: 12
            Kirigami.Icon {
                source: choiceRow.icon
                Layout.preferredWidth: 24
                Layout.preferredHeight: 24
                color: "#1d6fa5"
                isMask: true
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 1
                Label {
                    Layout.fillWidth: true
                    text: choiceRow.title
                    color: "#1f2d3a"
                    font.bold: true
                    wrapMode: Text.Wrap
                }
                Label {
                    Layout.fillWidth: true
                    visible: choiceRow.detail !== ""
                    text: choiceRow.detail
                    color: "#5d6d7e"
                    font.family: "monospace"
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                    elide: Text.ElideMiddle
                }
                Label {
                    Layout.fillWidth: true
                    visible: choiceRow.note !== ""
                    text: choiceRow.note
                    color: choiceRow.noteColor
                    font: Kirigami.Theme.smallFont
                    wrapMode: Text.Wrap
                }
            }
            RowLayout {
                id: rowButtons
                spacing: 8
            }
        }
    }

    // ================================================================ START
    ColumnLayout {
        Layout.fillWidth: true
        visible: view.mode === "start"
        spacing: 12

        Label {
            Layout.fillWidth: true
            text: qsTr("Where are your profiles? What you have now is never lost: it is set aside, and can be put back.")
            color: "#34495e"
            wrapMode: Text.Wrap
        }
        Repeater {
            model: [
                { id: "backup", icon: "document-save", title: qsTr("A data backup"),
                  sub: qsTr("Made by the Control Center, on a USB drive, a network folder or this disk") },
                { id: "system", icon: "drive-harddisk", title: qsTr("A system backup"),
                  sub: qsTr("The whole system, saved by the AstroArch backup tool, even on another kind of computer. Only its profiles, settings and images are restored here, never its system.") },
                { id: "disk", icon: "drive-removable-media", title: qsTr("An old AstroArch disk"),
                  sub: qsTr("The SD card or drive of your previous AstroArch, plugged in. It is only read, never changed.") },
                { id: "astromonitor", icon: "folder-cloud", title: "AstroMonitor",
                  sub: qsTr("The KStars, INDI and PHD2 profiles AstroMonitor keeps on the AstroArch server") }
            ]
            AbstractButton {
                id: tile
                required property var modelData
                Layout.fillWidth: true
                implicitHeight: tileRow.implicitHeight + 24
                focusPolicy: Qt.StrongFocus
                Accessible.role: Accessible.Button
                Accessible.name: modelData.title
                Accessible.description: modelData.sub
                Accessible.onPressAction: clicked()
                onClicked: view.start(modelData.id)
                background: Rectangle {
                    radius: 8
                    color: tile.hovered ? "#f4f8fb" : "#ffffff"
                    border.width: tile.visualFocus ? 2 : 1
                    border.color: tile.visualFocus || tile.hovered ? "#1d6fa5" : "#d3dbe2"
                }
                contentItem: RowLayout {
                    id: tileRow
                    spacing: 14
                    Kirigami.Icon {
                        Layout.leftMargin: 14
                        source: tile.modelData.icon
                        Layout.preferredWidth: 28
                        Layout.preferredHeight: 28
                        color: "#1d6fa5"
                        isMask: true
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Label {
                            Layout.fillWidth: true
                            text: tile.modelData.title
                            color: "#1f2d3a"
                            font.bold: true
                            wrapMode: Text.Wrap
                        }
                        Label {
                            Layout.fillWidth: true
                            text: tile.modelData.sub
                            color: "#5d6d7e"
                            font: Kirigami.Theme.smallFont
                            wrapMode: Text.Wrap
                        }
                    }
                    Kirigami.Icon {
                        Layout.rightMargin: 14
                        source: "go-next"
                        Layout.preferredWidth: 16
                        Layout.preferredHeight: 16
                        color: "#5d6d7e"
                        isMask: true
                    }
                }
            }
        }

        // Earlier restores: what they set aside can be put back
        Label {
            visible: view.asides.length > 0
            Layout.topMargin: 6
            text: qsTr("Earlier restores")
            color: "#1f2d3a"
            font.bold: true
        }
        RowsBox {
            visible: view.asides.length > 0
            Repeater {
                model: view.asides
                ColumnLayout {
                    id: asideRow
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true
                    spacing: 0
                    Separator { visible: asideRow.index > 0 }
                    ChoiceRow {
                        icon: "edit-undo"
                        title: qsTr("%1, from %2").arg(view.when(asideRow.modelData.time)).arg(view.labelText(asideRow.modelData.label))
                        detail: asideRow.modelData.folders.join(", ")
                        note: qsTr("%n file(s) restored. What they replaced is kept here.", "", asideRow.modelData.files)
                        CardButton {
                            enabled: !restore.busy
                            text: qsTr("Put back what was there before")
                            Accessible.name: text + " · " + view.when(asideRow.modelData.time)
                            onClicked: restore.putBack(JSON.stringify(asideRow.modelData.folders))
                        }
                    }
                }
            }
        }
    }

    // =============================================================== WIZARD
    ColumnLayout {
        Layout.fillWidth: true
        visible: view.mode === "wizard"
        spacing: 12

        // "Step 2 of 3" and the segmented bar, as the Data backup page
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Label {
                text: qsTr("Step %1 of %2").arg(view.step).arg(view.steps.length)
                color: "#1d6fa5"
                font.bold: true
            }
            Label {
                Layout.fillWidth: true
                text: view.steps[view.step - 1]
                color: "#1f2d3a"
                font.bold: true
                elide: Text.ElideRight
            }
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 4
            Accessible.ignored: true
            Repeater {
                model: view.steps.length
                Rectangle {
                    required property int index
                    Layout.fillWidth: true
                    implicitHeight: 6
                    radius: 3
                    color: index < view.step ? "#1d6fa5" : "#c9d3db"
                }
            }
        }

        // ---- Step 1, data backup: the places, the systems in them, their snapshots
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 1 && view.kind === "backup"
            spacing: 10
            Label {
                Layout.fillWidth: true
                text: qsTr("Plug in the drive of the backup, or connect its network folder. Then choose the date to go back to.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            Label {
                Layout.fillWidth: true
                visible: view.places.length === 0
                text: qsTr("No data backup found yet. The list follows the drives you plug in.")
                color: "#5d6d7e"
                wrapMode: Text.Wrap
            }
            RowsBox {
                visible: view.places.length > 0
                Repeater {
                    model: view.places
                    ColumnLayout {
                        id: place
                        required property var modelData
                        required property int index
                        readonly property var found: view.systems[modelData.path] || []
                        Layout.fillWidth: true
                        spacing: 0
                        Separator { visible: place.index > 0 }
                        ChoiceRow {
                            icon: ({ internal: "drive-harddisk", usb: "drive-removable-media-usb", nas: "folder-network" })[place.modelData.kind] || "folder"
                            title: ({ internal: qsTr("This disk"), usb: qsTr("USB drive %1"), nas: qsTr("Network folder %1") })[place.modelData.kind].arg(place.modelData.label || "")
                            detail: place.modelData.path || place.modelData.detail || ""
                            note: place.modelData.state === "not_mounted" ? qsTr("Not mounted")
                                : (place.found.length === 0 ? qsTr("No data backup here") : qsTr("%n backup(s) here", "", place.found.length))
                            noteColor: place.found.length > 0 ? "#1e7a45" : "#5d6d7e"
                            CardButton {
                                visible: place.modelData.state === "not_mounted"
                                enabled: !dataBackend.busy
                                text: qsTr("Mount")
                                onClicked: dataBackend.mount(place.modelData.device)
                            }
                        }
                        // One AstroArch each (the folder named after it), with its snapshots
                        Repeater {
                            model: place.found
                            ColumnLayout {
                                id: system
                                required property var modelData
                                readonly property bool open: !!view.chosenSystem && view.chosenSystem.folder === modelData.folder
                                Layout.fillWidth: true
                                Layout.leftMargin: 36
                                spacing: 2
                                ChoiceRow {
                                    icon: "computer"
                                    title: system.modelData.id + (system.modelData.thisSystem ? " · " + qsTr("this AstroArch") : "")
                                    note: system.modelData.snapshots.length > 0
                                          ? qsTr("Settings from %1 · %n date(s) to choose from", "", system.modelData.snapshots.length)
                                              .arg(view.snapshotText(system.modelData.snapshots[0]))
                                          : qsTr("Images only")
                                    CardButton {
                                        visible: !system.open
                                        text: qsTr("Choose")
                                        Accessible.name: text + " · " + system.modelData.id
                                        onClicked: view.chooseSnapshot(system.modelData, system.modelData.snapshots[0] || "")
                                    }
                                }
                                Flow {
                                    visible: system.open && system.modelData.snapshots.length > 1
                                    Layout.fillWidth: true
                                    Layout.leftMargin: 50
                                    Layout.bottomMargin: 8
                                    spacing: 6
                                    Repeater {
                                        model: system.open ? system.modelData.snapshots : []
                                        CardButton {
                                            required property var modelData
                                            text: view.snapshotText(modelData)
                                            checked: view.snapshot === modelData
                                            Accessible.role: Accessible.RadioButton
                                            Accessible.checked: checked
                                            onClicked: view.chooseSnapshot(system.modelData, modelData)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            CardButton {
                visible: !view.shareForm
                text: qsTr("Connect a network folder…")
                onClicked: view.shareForm = true
            }
            ShareForm {
                id: shareBox
                visible: view.shareForm
                manager: dataBackend
                // Its message under its Connect button
                message: view.message
                warning: view.warning
                onCancelled: view.shareForm = false
                onSubmitted: view.say("")
            }
        }

        // ---- Step 1, system backup: its places, the one opened, its snapshots
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 1 && view.kind === "system"
            spacing: 10
            Label {
                Layout.fillWidth: true
                text: qsTr("Plug in the drive of the system backup if it is on one, open the backup (read-only: nothing in it is changed), then choose the date to go back to.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            Label {
                Layout.fillWidth: true
                visible: view.sysPlaces.length === 0
                text: qsTr("No system backup found yet. The list follows the drives you plug in.")
                color: "#5d6d7e"
                wrapMode: Text.Wrap
            }
            RowsBox {
                visible: view.sysPlaces.length > 0
                Repeater {
                    model: view.sysPlaces
                    ColumnLayout {
                        id: sysPlace
                        required property var modelData
                        required property int index
                        readonly property bool open: view.sysDest !== "" && view.sysDest === modelData.path
                        Layout.fillWidth: true
                        spacing: 0
                        Separator { visible: sysPlace.index > 0 }
                        ChoiceRow {
                            icon: ({ internal: "drive-harddisk", usb: "drive-removable-media-usb" })[sysPlace.modelData.kind] || "folder"
                            title: ({ internal: qsTr("This disk"), usb: qsTr("USB drive %1").arg(sysPlace.modelData.label), folder: qsTr("Folder") })[sysPlace.modelData.kind]
                            detail: sysPlace.modelData.path
                            note: sysPlace.modelData.state === "not_mounted" ? qsTr("Not mounted")
                                : sysPlace.open && view.sysLoading ? qsTr("Reading the backup… Your password is asked.")
                                : sysPlace.open ? qsTr("%n snapshot(s)", "", view.sysSnapshots.length)
                                : qsTr("System backup")
                            noteColor: sysPlace.modelData.state === "not_mounted" ? "#5d6d7e" : "#1e7a45"
                            CardButton {
                                visible: sysPlace.modelData.state === "not_mounted"
                                enabled: !systemBackup.isRunning
                                text: qsTr("Mount")
                                onClicked: systemBackup.mount_media(sysPlace.modelData.device)
                            }
                            CardButton {
                                visible: sysPlace.modelData.state !== "not_mounted" && !sysPlace.open
                                enabled: !view.sysLoading && !restore.busy
                                text: qsTr("Open read-only")
                                Accessible.name: text + " · " + sysPlace.modelData.path
                                onClicked: view.openSystem(sysPlace.modelData.path)
                            }
                        }
                        // Its snapshots, newest first: any of them, whatever system it was made on
                        Repeater {
                            model: sysPlace.open ? view.sysSnapshots : []
                            ChoiceRow {
                                id: sysSnap
                                required property var modelData
                                Layout.leftMargin: 36
                                icon: "computer"
                                chosen: view.snapshot === modelData.name && !!view.source
                                title: view.snapshotText(modelData.name)
                                note: view.archNote(modelData)
                                noteColor: modelData.foreign ? "#9a5b00" : "#5d6d7e"
                                CardButton {
                                    visible: !sysSnap.chosen
                                    enabled: !restore.busy
                                    text: qsTr("Choose")
                                    Accessible.name: text + " · " + sysSnap.title
                                    onClicked: view.chooseSystemSnapshot(sysSnap.modelData)
                                }
                            }
                        }
                    }
                }
            }
        }

        // ---- Step 1, old disk: its partitions, opened read-only
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 1 && view.kind === "disk"
            spacing: 10
            Label {
                Layout.fillWidth: true
                text: qsTr("Plug in the SD card (with a card reader) or the drive of your old AstroArch, then open its system partition. It is opened read-only: nothing on it is changed, and nothing of its system is copied, only your profiles and images.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            Label {
                Layout.fillWidth: true
                visible: view.disks.length === 0
                text: qsTr("No disk found yet. The list follows the disks you plug in.")
                color: "#5d6d7e"
                wrapMode: Text.Wrap
            }
            RowsBox {
                visible: view.disks.length > 0
                Repeater {
                    model: view.disks
                    ColumnLayout {
                        id: disk
                        required property var modelData
                        required property int index
                        readonly property bool chosen: !!view.source && view.source.root === modelData.opened && modelData.opened !== ""
                        Layout.fillWidth: true
                        spacing: 0
                        Separator { visible: disk.index > 0 }
                        ChoiceRow {
                            icon: "drive-removable-media"
                            chosen: disk.chosen
                            title: (disk.modelData.label || disk.modelData.device) + " · " + view.fmt(disk.modelData.size)
                            detail: disk.modelData.device + " · " + disk.modelData.fstype
                            note: disk.chosen
                                  ? (view.about.found
                                     ? [view.about.version ? "AstroArch " + view.about.version : qsTr("AstroArch"),
                                        qsTr("user %1").arg(view.about.user),
                                        view.about.kiosk ? qsTr("with a kiosk desktop") : ""].filter(t => t).join(" · ")
                                     : qsTr("No AstroArch home folder on this partition"))
                                  : (disk.modelData.astroarch ? qsTr("Looks like an AstroArch system") : "")
                            noteColor: disk.chosen && view.about.found ? "#1e7a45" : "#5d6d7e"
                            CardButton {
                                visible: !disk.chosen
                                enabled: !restore.busy
                                text: restore.busy && restore.stage === "auth" ? qsTr("Waiting for the password…") : qsTr("Open read-only")
                                Accessible.name: qsTr("Open read-only") + " · " + disk.modelData.device
                                onClicked: {
                                    view.say("")
                                    restore.diskName = disk.modelData.label || disk.modelData.device
                                    if (disk.modelData.opened !== "")
                                        view.openSource({ kind: "disk", root: disk.modelData.opened, name: restore.diskName })
                                    else
                                        restore.mountDisk(disk.modelData.device)
                                }
                            }
                        }
                    }
                }
            }
        }

        // ---- Step 1, AstroMonitor: the token, then the download
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 1 && view.kind === "astromonitor"
            spacing: 10
            Label {
                Layout.fillWidth: true
                text: qsTr("AstroMonitor keeps your profiles under your token, the one the Telegram bot @AstroMonitorBot gave you with /register. On a new AstroArch, paste it here.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                Label { text: qsTr("Token"); color: "#4f5a64"; font.bold: true }
                CardField {
                    id: tokenField
                    Layout.fillWidth: true
                    Accessible.name: qsTr("AstroMonitor token")
                    onTextChanged: if (view.source) view.source = null
                    onAccepted: view.fetch()
                }
                CardButton {
                    highlighted: !view.source
                    enabled: tokenField.text.trim() !== "" && !restore.busy
                    text: restore.busy ? qsTr("Downloading…") : qsTr("Download")
                    onClicked: view.fetch()
                }
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("It holds KStars profiles, equipment and settings, INDI driver settings and the PHD2 profile, as of your last “Take Backup” in AstroMonitor.")
                color: "#5d6d7e"
                font: Kirigami.Theme.smallFont
                wrapMode: Text.Wrap
            }
        }

        // ---- Step 2: what to restore
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 2
            spacing: 10
            Label {
                Layout.fillWidth: true
                text: view.kind === "backup" ? qsTr("From the backup of %1, as it was on %2.").arg(view.chosenSystem ? view.chosenSystem.id : "").arg(view.snapshotText(view.snapshot))
                    : view.kind === "system" ? qsTr("From the system backup, as it was on %1.").arg(view.snapshotText(view.snapshot))
                    : view.kind === "disk" ? qsTr("From the disk %1.").arg(restore.diskName)
                    : qsTr("From your AstroMonitor backup.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            SelectionList {
                Layout.fillWidth: true
                groups: view.groups
                excluded: view.excluded
                sizes: view.sizes
                missingText: qsTr("Not in this backup")
                childMissingText: qsTr("Not in this backup")
                onEdited: (excluded) => view.excluded = excluded
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("Each file replaced is first moved to ~/.local/share/astroarch/restore-DATE: this page can put it back. Images are only added: those already here are kept.")
                color: "#5d6d7e"
                font: Kirigami.Theme.smallFont
                wrapMode: Text.Wrap
            }
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: view.running !== ""
                type: Kirigami.MessageType.Warning
                text: qsTr("Close %1 first: they would write over the restored files.").arg(view.appNames(view.running))
            }
        }

        // ---- Step 3: the restore
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 3
            spacing: 10
            Label {
                Layout.fillWidth: true
                visible: !view.result
                text: restore.stage === "auth" ? qsTr("Waiting for your password…")
                    : restore.progress >= 0 ? qsTr("Restoring… %1%").arg(restore.progress) : qsTr("Restoring…")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            ProgressBar {
                Layout.fillWidth: true
                visible: !view.result
                indeterminate: restore.progress < 0
                from: 0
                to: 100
                value: Math.max(0, restore.progress)
            }
            RowLayout {
                visible: !!view.result
                spacing: 8
                Kirigami.Icon {
                    source: "checkmark"
                    color: "#1e7a45"
                    isMask: true
                    Layout.preferredWidth: 18
                    Layout.preferredHeight: 18
                }
                Label {
                    Layout.fillWidth: true
                    text: qsTr("Restored. KStars, PHD2 and the Equipment page now find your profiles.")
                    color: "#1e7a45"
                    wrapMode: Text.Wrap
                }
            }
            Label {
                Layout.fillWidth: true
                visible: !!view.result && view.result.aside.length > 0
                text: qsTr("What was there before is kept in %1.").arg(view.result ? view.result.aside.join(", ") : "")
                color: "#34495e"
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
            }
            Label {
                Layout.fillWidth: true
                visible: !!view.result && view.result.skipped.some(s => s.startsWith("shared:") || s.startsWith("link:"))
                text: qsTr("Some kiosk profiles were left as they are here: the Kiosk page sets whether the kiosk shares the profiles of the main desktop.")
                color: "#5d6d7e"
                font: Kirigami.Theme.smallFont
                wrapMode: Text.Wrap
            }
        }

    }

    // The message and the wizard's buttons, pinned under the page (Main.qml's navHost)
    ColumnLayout {
        id: navBar
        Layout.fillWidth: true
        spacing: 10
        visible: view.visible && view.navShown
        Component.onCompleted: if (view.navHost) parent = view.navHost

        // Not while the network folder form shows it under its Connect
        Kirigami.InlineMessage {
            id: navMessage
            Layout.fillWidth: true
            visible: view.message !== "" && !view.formOpen
            type: view.warning ? Kirigami.MessageType.Warning : Kirigami.MessageType.Information
            text: view.message
        }
        // Back / Next: the only navigation while the wizard is open
        RowLayout {
            Layout.fillWidth: true
            visible: view.mode === "wizard"
            spacing: 10
            // Hidden while the network folder form shows its own Cancel
            CardButton {
                visible: !(view.step === 3 && !!view.result) && !view.formOpen
                enabled: !(view.step === 3 && restore.busy)
                text: view.step === 1 ? qsTr("Cancel") : qsTr("Back")
                onClicked: view.back()
            }
            Item { Layout.fillWidth: true }
            CardButton {
                visible: view.step === 3 && !!view.result && view.result.aside.length > 0
                enabled: !restore.busy
                text: qsTr("Put back what was there before")
                onClicked: restore.putBack(JSON.stringify(view.result.aside))
            }
            // Plain while a form of the step has the action (Connect, Download)
            CardButton {
                visible: view.step < 3
                highlighted: !view.formOpen && !(view.step === 1 && view.kind === "astromonitor" && !view.source)
                enabled: view.step === 1 ? !!view.source && !restore.busy
                                         : view.running === "" && !restore.busy
                text: view.step === 2 ? qsTr("Restore") : qsTr("Next")
                onClicked: view.next()
            }
            CardButton {
                visible: view.step === 3 && !!view.result
                highlighted: true
                text: qsTr("Finish")
                onClicked: view.leave()
            }
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import AstroArchControlCenter

// The Data backup page: the two profiles of the backup tool, in the page.
//   Data and settings: what (catalog of crates/astroarch-backup/data/data-profile.toml),
//     where, every 2 hours; status and "Back up now". Logic: DataBackup (src/data_backup.rs)
//   System: the backup tool's own BackupManager (its bridge.rs, the same file)
// A wizard of three steps for the data backup, in the style of the approved design D
// ("Step 2 of 3" and a segmented bar), all inside the page's light card
ColumnLayout {
    id: view

    // The setup changed: the Control Center reads the system again
    signal setupChanged()
    // Where the wizard's buttons go, pinned under the page (Main.qml); null: in the view
    property Item navHost: null
    readonly property bool navShown: view.profile === "data" && view.mode === "wizard"
    // Another step or view: the page goes back to its top
    signal scrollToTop()
    // Shows `item` whole in the page: a message, a list just opened (Main.qml scrolls)
    signal reveal(Item item)

    spacing: 14


    // ------------------------------------------------------------------ state
    property string profile: "data"        // "data" | "system"
    property string mode: "overview"       // "overview" | "wizard"
    property int step: 1                   // 1 what, 2 where, 3 when
    property var groups: []                // DataBackup.rows()
    property var excluded: ({})            // key -> true
    property var added: []
    property var sizes: ({})               // key -> bytes
    property var openRows: ({})            // item id -> subfolders shown
    property int keep: 14
    property bool scheduleOn: true
    property bool firstNow: true
    property var dest: ({})                // chosen destination {kind, path, label}
    property var saved: ({})               // DataBackup.config()
    property var dests: []
    property var status: ({})
    property var timerState: ({})
    property string message: ""
    property bool warning: false
    property string newFolder: ""
    property double runStarted: 0          // "Back up now" pressed (seconds)

    onStepChanged: view.scrollToTop()
    onModeChanged: view.scrollToTop()
    // A new message, or a list of the system backup opened: shown in the page
    onMessageChanged: if (view.message !== "") view.revealSoon(null)
    onSysStateChanged: {
        if (view.sysState === "choose")
            view.revealSoon(sysPlaces)
        else if (view.sysState === "folders")
            view.revealSoon(sysFolders)
    }

    readonly property bool configured: !!saved.configured
    // Narrow window: the buttons of a row go under its text
    readonly property bool narrow: width < 560
    // One primary button at a time, then not the footer's (Main.qml): "Set up the data
    // backup…", the wizard's Next, "Done" of the folders left out
    readonly property bool ownsPrimary: view.profile === "data" ? (view.mode === "wizard" || !view.configured)
                                                                : view.sysState === "folders"
    readonly property var steps: [qsTr("What is included"), qsTr("Where to keep the backups"), qsTr("When")]

    function fmt(bytes) {
        return bytes === undefined || bytes === null ? "—" : Qt.locale().formattedDataSize(bytes)
    }
    function when(seconds) {
        return Qt.locale().toString(new Date(seconds * 1000), Locale.ShortFormat)
    }
    function say(text, isWarning) {
        view.warning = !!isWarning
        view.message = text
    }
    // `item`, or the message shown (null), once laid out: a message unfolds
    function revealSoon(item) {
        revealTimer.item = item
        revealTimer.restart()
    }

    function reload() {
        const c = JSON.parse(backend.config())
        const ex = {}
        for (const k of c.excluded || [])
            ex[k] = true
        view.saved = c
        view.excluded = ex
        view.added = c.added || []
        view.keep = c.keep || 14
        view.dest = c.destination || {}
        view.groups = JSON.parse(backend.rows())
        backend.requestSizes()
        view.refreshStatus()
    }
    function refreshStatus() {
        view.status = JSON.parse(backend.status())
        view.timerState = JSON.parse(backend.schedule())
    }
    function refreshDests() {
        view.dests = JSON.parse(backend.destinations(view.selectedBytes()))
    }

    // What the data backup copies (the list of step 1 holds the selection)
    function selectedBytes() {
        return dataList.selectedBytes()
    }
    function selectedCount() {
        return dataList.selectedCount() + view.added.length
    }

    // ---------------------------------------------------------- destinations
    function destName(d) {
        switch (d.kind) {
        case "internal": return qsTr("This disk")
        case "usb": return qsTr("USB drive %1").arg(d.label || "")
        case "nas": return qsTr("Network folder %1").arg(d.label || "")
        case "syncthing": return qsTr("Your computer, with Syncthing")
        }
        return d.path || ""
    }
    function destNote(d) {
        if (d.kind === "internal")
            return qsTr("Ready now. It protects from mistakes, not from a broken SD card or disk.")
        if (d.kind === "syncthing")
            return qsTr("Kept on this disk and sent to your computer, which keeps the old versions.")
        return ""
    }
    function stateText(d) {
        return ({
            ready: qsTr("%1 free").arg(view.fmt(d.free)),
            too_small: qsTr("Not enough space: %1 free").arg(view.fmt(d.free)),
            not_mounted: qsTr("Not mounted"),
            fat32: qsTr("FAT32: cannot be used"),
            nohistory: qsTr("exFAT or NTFS keeps no history: cannot be used"),
            readonly: qsTr("Your user cannot write here")
        })[d.state] || d.state
    }
    function sameDest(a, b) {
        return !!a && !!b && a.kind === b.kind && a.path === b.path
    }
    // A place the backups can go to now (one too small is listed, not usable)
    function destUsable(d) {
        return d.state === "ready"
    }
    // The chosen place is in the list and usable
    function destOk() {
        return view.dests.some(d => view.sameDest(d, view.dest) && view.destUsable(d))
    }

    // ---------------------------------------------------------------- actions
    function startWizard(atStep) {
        view.reload()
        view.mode = "wizard"
        view.step = atStep
        view.scheduleOn = view.configured ? !!view.timerState.enabled : true
        view.firstNow = !view.configured
        view.say("")
        if (atStep === 2)
            view.refreshDests()
    }
    function apply() {
        const ex = Object.keys(view.excluded).filter(k => view.excluded[k])
        view.say(qsTr("Saving…"))
        backend.apply(JSON.stringify({
            excluded: ex, added: view.added, keep: view.keep, schedule: view.scheduleOn,
            destination: { kind: view.dest.kind, path: view.dest.path, label: view.dest.label || "" }
        }))
    }
    function runNow() {
        const r = backend.runNow()
        view.runStarted = Date.now() / 1000
        if (r === "started")
            view.say(qsTr("Backing up now. You can close the window: it goes on."))
        else if (r === "running")
            view.say(qsTr("A backup is already running."))
        else
            view.say(qsTr("The backup tool (astroarch-backup) is not installed."), true)
        view.refreshStatus()
    }
    function addFolder(path) {
        const p = path.trim().replace(/\/+$/, "")
        const problem = backend.checkFolder(p)
        if (problem !== "") {
            view.say(({ missing: qsTr("%1 is not a folder on this system.").arg(p),
                        too_wide: qsTr("%1 is too wide: choose a folder inside it.").arg(p),
                        destination: qsTr("%1 holds the backups themselves.").arg(p) })[problem] || problem, true)
            return
        }
        if (!view.added.includes(p))
            view.added = view.added.concat([p])
        view.newFolder = ""
        view.say("")
    }

    function statusText() {
        const s = view.status
        if (s.active)
            return s.stage === "data" ? qsTr("Backing up now: images and data…") : qsTr("Backing up now: settings…")
        switch (s.state) {
        case "ok": return qsTr("Done · %1").arg(view.when(s.time))
        case "partial": return qsTr("Done · %1 · some files could not be read").arg(view.when(s.time))
        case "failed":
            return s.reason === "missing" ? qsTr("Failed · %1: the destination is not there. Is the drive plugged in, is the NAS on?").arg(view.when(s.time))
                                          : qsTr("Failed · %1: %2").arg(view.when(s.time)).arg(s.message || "")
        case "skipped": return qsTr("Skipped · %1: a session was in progress").arg(view.when(s.time))
        case "running": return qsTr("Stopped before the end · %1").arg(view.when(s.time))
        }
        return qsTr("No backup yet")
    }

    DataBackup {
        id: backend
        onSizeReady: (key, bytes) => {
            const s = Object.assign({}, view.sizes)
            s[key] = bytes
            view.sizes = s
        }
        onMounted: (device, mountpoint, error) => {
            if (error !== "")
                view.say(qsTr("Could not mount the drive: %1").arg(error), true)
            view.refreshDests()
        }
        onApplied: (ok, error) => {
            if (!ok) {
                view.say(error === "auth" ? qsTr("Authentication cancelled or refused.")
                         : qsTr("Could not turn on the backup: %1").arg(error), true)
                return
            }
            const first = view.firstNow
            view.reload()
            view.mode = "overview"
            view.say(qsTr("The data backup is set up."))
            if (first)
                view.runNow()
            view.setupChanged()
        }
    }

    Component.onCompleted: view.reload()

    // The status follows a running backup; the drive list follows drives plugged in
    Timer {
        interval: 3000
        repeat: true
        running: view.visible && view.profile === "data"
        onTriggered: {
            view.refreshStatus()
            // The backup asked with "Back up now" has finished: the status row says how
            if (view.runStarted > 0 && !view.status.active && view.status.time >= Math.floor(view.runStarted)
                    && view.status.state !== "running") {
                view.runStarted = 0
                view.say("")
            }
        }
    }
    Timer {
        interval: 4000
        repeat: true
        running: view.visible && view.mode === "wizard" && view.step === 2 && !backend.busy
        onTriggered: view.refreshDests()
    }

    Timer {
        id: revealTimer
        property Item item: null
        interval: 300
        onTriggered: {
            const target = revealTimer.item || [wizardMessage, overviewMessage, systemMessage].find(m => m.visible)
            // Not when the page is not shown (the view is kept while another page is)
            if (target && target.visible)
                view.reveal(target)
        }
    }

    // --------------------------------------------------------- components
    // The view's message, shown where the user is: above the buttons that go on, in the
    // form that asked. `here`: this one is the place now
    component ViewMessage: Kirigami.InlineMessage {
        property bool here: true
        Layout.fillWidth: true
        visible: here && view.message !== ""
        type: view.warning ? Kirigami.MessageType.Warning : Kirigami.MessageType.Information
        text: view.message
    }

    component Separator: Rectangle {
        Layout.fillWidth: true
        implicitHeight: 1
        color: "#e1e6ea"
    }

    // One value row: label, value and a smaller line, buttons on the right
    component InfoRow: ColumnLayout {
        id: info
        property string label
        property string value
        property string sub
        property color valueColor: "#1f2d3a"
        default property alias buttons: actions.data
        Layout.fillWidth: true
        spacing: 0
        GridLayout {
            Layout.fillWidth: true
            Layout.margins: 12
            columns: view.narrow ? 1 : 3
            columnSpacing: 16
            rowSpacing: 8
            Label {
                Layout.preferredWidth: view.narrow ? -1 : 130
                text: info.label
                color: "#4f5a64"
                font.bold: true
                wrapMode: Text.Wrap
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label {
                    Layout.fillWidth: true
                    text: info.value
                    color: info.valueColor
                    wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                }
                Label {
                    Layout.fillWidth: true
                    visible: info.sub !== ""
                    text: info.sub
                    color: "#5d6d7e"
                    font: Kirigami.Theme.smallFont
                    wrapMode: Text.Wrap
                }
            }
            RowLayout {
                id: actions
                spacing: 8
            }
        }
    }

    // ------------------------------------------------------- profile choice
    // The two profiles of the backup tool
    RowLayout {
        Layout.fillWidth: true
        spacing: 10
        enabled: view.mode === "overview" && !system.isRunning

        Repeater {
            model: [
                { id: "data", title: qsTr("Data and settings"), sub: qsTr("KStars, PHD2, INDI, kiosk and your images · every 2 hours") },
                { id: "system", title: qsTr("System"), sub: qsTr("The whole system, to undo an update · on request and before updating") }
            ]
            AbstractButton {
                id: choice
                required property var modelData
                readonly property bool current: view.profile === modelData.id
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                implicitHeight: choiceColumn.implicitHeight + 20
                focusPolicy: Qt.StrongFocus
                Accessible.role: Accessible.RadioButton
                Accessible.checkable: true
                Accessible.checked: current
                Accessible.name: modelData.title
                Accessible.onPressAction: clicked()
                onClicked: {
                    view.profile = modelData.id
                    view.say("")
                    if (modelData.id === "system")
                        system.load_config()
                }
                background: Rectangle {
                    radius: 8
                    color: choice.current ? "#e3f0f8" : (choice.hovered ? "#f4f8fb" : "#ffffff")
                    border.width: choice.current || choice.visualFocus ? 2 : 1
                    border.color: choice.current || choice.visualFocus ? "#1d6fa5" : "#d3dbe2"
                }
                contentItem: ColumnLayout {
                    id: choiceColumn
                    spacing: 2
                    Label {
                        Layout.fillWidth: true
                        Layout.leftMargin: 12
                        Layout.rightMargin: 12
                        text: choice.modelData.title
                        color: "#1f2d3a"
                        font.bold: true
                        wrapMode: Text.Wrap
                    }
                    Label {
                        Layout.fillWidth: true
                        Layout.leftMargin: 12
                        Layout.rightMargin: 12
                        text: choice.modelData.sub
                        color: "#5d6d7e"
                        font: Kirigami.Theme.smallFont
                        wrapMode: Text.Wrap
                    }
                }
            }
        }
    }

    // =============================================================== DATA
    // ----------------------------------------------------------- overview
    ColumnLayout {
        Layout.fillWidth: true
        visible: view.profile === "data" && view.mode === "overview"
        spacing: 12

        Label {
            Layout.fillWidth: true
            visible: !view.configured
            text: qsTr("Not set up yet. Choose what to keep and where: it takes a minute, and the first backup starts right away.")
            color: "#34495e"
            wrapMode: Text.Wrap
        }
        CardButton {
            visible: !view.configured
            highlighted: true
            text: qsTr("Set up the data backup…")
            onClicked: view.startWizard(1)
        }

        RowsBox {
            visible: view.configured
            InfoRow {
                label: qsTr("Last backup")
                value: view.statusText()
                valueColor: view.status.state === "failed" && !view.status.active ? "#8a1c16" : "#1f2d3a"
                sub: view.status.lastOk && view.status.state !== "ok" && !view.status.active
                     ? qsTr("Last complete one: %1").arg(view.when(view.status.lastOk)) : ""
                CardButton {
                    enabled: !view.status.active
                    text: view.status.active ? qsTr("Backing up…") : qsTr("Back up now")
                    onClicked: view.runNow()
                }
            }
            Separator {}
            InfoRow {
                label: qsTr("Schedule")
                value: !view.timerState.installed ? qsTr("Not available: the timer is not installed")
                     : view.timerState.enabled
                       ? (view.timerState.next > 0 ? qsTr("Every 2 hours · next at %1").arg(Qt.formatTime(new Date(view.timerState.next * 1000), "HH:mm"))
                                                   : qsTr("Every 2 hours"))
                       : qsTr("Off: only when you press “Back up now”")
                sub: qsTr("Skipped while KStars and INDI run a session, so imaging is never slowed down")
                CardButton {
                    visible: !!view.timerState.installed
                    text: view.timerState.enabled ? qsTr("Turn off") : qsTr("Turn on")
                    onClicked: {
                        const error = backend.setSchedule(!view.timerState.enabled)
                        view.say(error === "" ? "" : qsTr("Could not change the schedule: %1").arg(error), error !== "")
                        view.refreshStatus()
                        view.setupChanged()
                    }
                }
            }
            Separator {}
            InfoRow {
                label: qsTr("Where")
                value: view.destName(view.saved.destination || {})
                sub: (view.saved.destination || {}).path || ""
                CardButton {
                    text: qsTr("Change…")
                    Accessible.name: qsTr("Change where the backups go")
                    onClicked: view.startWizard(2)
                }
            }
            Separator {}
            InfoRow {
                label: qsTr("What")
                value: qsTr("%n item(s) · %1", "", view.selectedCount()).arg(view.fmt(view.selectedBytes()))
                sub: qsTr("Settings keep their last %n version(s); images are only added to", "", view.keep)
                CardButton {
                    text: qsTr("Change…")
                    Accessible.name: qsTr("Change what is backed up")
                    onClicked: view.startWizard(1)
                }
            }
        }
        ViewMessage { id: overviewMessage }
    }

    // ------------------------------------------------------------- wizard
    ColumnLayout {
        Layout.fillWidth: true
        visible: view.profile === "data" && view.mode === "wizard"
        spacing: 12

        // "Step 2 of 3" and the segmented bar
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
            Label {
                visible: view.step < view.steps.length
                text: qsTr("Next: %1").arg(view.step < view.steps.length ? view.steps[view.step] : "")
                color: "#5d6d7e"
                font: Kirigami.Theme.smallFont
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

        // ---- Step 1: what is included
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 1
            spacing: 10

            RowLayout {
                Layout.fillWidth: true
                Label {
                    Layout.fillWidth: true
                    text: qsTr("The important settings of your astronomy apps are already selected. Only the first backup copies everything: the next ones copy what changed.")
                    color: "#34495e"
                    wrapMode: Text.Wrap
                }
                Label {
                    text: qsTr("Selected: %1").arg(view.fmt(view.selectedBytes()))
                    color: "#1f2d3a"
                    font.bold: true
                }
            }

            SelectionList {
                id: dataList
                Layout.fillWidth: true
                groups: view.groups
                excluded: view.excluded
                sizes: view.sizes
                onEdited: (excluded) => view.excluded = excluded
            }

            // Folders the user added
            RowsBox {
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 14
                    Layout.topMargin: 10
                    text: qsTr("Other folders")
                    color: "#5d6d7e"
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                    font.bold: true
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 0.5
                }
                Repeater {
                    model: view.added
                    RowLayout {
                        id: addedRow
                        required property string modelData
                        Layout.fillWidth: true
                        Layout.leftMargin: 14
                        Layout.rightMargin: 14
                        Layout.topMargin: 4
                        spacing: 10
                        Label {
                            Layout.fillWidth: true
                            text: addedRow.modelData
                            color: "#1f2d3a"
                            font.family: "monospace"
                            elide: Text.ElideMiddle
                        }
                        CardButton {
                            text: qsTr("Remove")
                            Accessible.name: qsTr("Remove %1").arg(addedRow.modelData)
                            onClicked: view.added = view.added.filter(a => a !== addedRow.modelData)
                        }
                    }
                }
                // A path typed, or one of the folders of the home
                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 14
                    Layout.topMargin: 6
                    spacing: 8
                    TextField {
                        id: folderField
                        Layout.fillWidth: true
                        text: view.newFolder
                        onTextEdited: view.newFolder = text
                        placeholderText: qsTr("A folder to add, e.g. %1").arg(JSON.parse(backend.homeFolders())[0] || "/home")
                        placeholderTextColor: "#7a828a"
                        color: "#232629"
                        font.family: "monospace"
                        Accessible.name: qsTr("Folder to add")
                        onAccepted: view.addFolder(text)
                        background: Rectangle {
                            radius: 4
                            color: "#ffffff"
                            border.width: folderField.activeFocus ? 2 : 1
                            border.color: folderField.activeFocus ? "#3daee9" : "#9aa4ad"
                        }
                    }
                    CardButton {
                        text: qsTr("Add")
                        enabled: view.newFolder.trim() !== ""
                        Accessible.name: qsTr("Add this folder")
                        onClicked: view.addFolder(view.newFolder)
                    }
                }
                Flow {
                    Layout.fillWidth: true
                    Layout.leftMargin: 14
                    Layout.rightMargin: 14
                    Layout.bottomMargin: 12
                    spacing: 6
                    Repeater {
                        model: JSON.parse(backend.homeFolders()).filter(f => !view.added.includes(f))
                        CardButton {
                            required property string modelData
                            text: "+ " + modelData.split("/").pop()
                            topPadding: 4
                            bottomPadding: 4
                            implicitHeight: 32
                            Accessible.name: qsTr("Add %1").arg(modelData)
                            onClicked: view.addFolder(modelData)
                        }
                    }
                }
            }
        }

        // ---- Step 2: where
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 2
            spacing: 10

            Label {
                Layout.fillWidth: true
                text: qsTr("About %1 to copy the first time. The list follows the drives you plug in.").arg(view.fmt(view.selectedBytes()))
                color: "#34495e"
                wrapMode: Text.Wrap
            }

            RowsBox {
                Repeater {
                    model: view.dests
                    ColumnLayout {
                        id: place
                        required property var modelData
                        required property int index
                        readonly property bool chosen: view.sameDest(view.dest, modelData)
                        readonly property bool usable: view.destUsable(modelData)
                        Layout.fillWidth: true
                        spacing: 0
                        Separator { visible: place.index > 0 }
                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: placeRow.implicitHeight + 20
                            color: place.chosen ? "#e3f0f8" : "transparent"
                            border.width: place.chosen ? 2 : 0
                            border.color: "#1d6fa5"
                            radius: 6
                            RowLayout {
                                id: placeRow
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.leftMargin: 14
                                anchors.rightMargin: 14
                                spacing: 12
                                Kirigami.Icon {
                                    source: ({ internal: "drive-harddisk", usb: "drive-removable-media-usb", nas: "folder-network",
                                               syncthing: "computer" })[place.modelData.kind] || "folder"
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
                                        text: view.destName(place.modelData)
                                        color: "#1f2d3a"
                                        font.bold: true
                                        wrapMode: Text.Wrap
                                    }
                                    Label {
                                        Layout.fillWidth: true
                                        text: place.modelData.detail || place.modelData.path
                                        color: "#5d6d7e"
                                        font.family: "monospace"
                                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                                        elide: Text.ElideMiddle
                                    }
                                    Label {
                                        Layout.fillWidth: true
                                        text: view.stateText(place.modelData)
                                              + (place.modelData.hasBackup ? " · " + qsTr("a backup is already there") : "")
                                        color: place.modelData.state === "ready" ? "#1e7a45" : "#8a4b00"
                                        font: Kirigami.Theme.smallFont
                                        wrapMode: Text.Wrap
                                    }
                                    Label {
                                        Layout.fillWidth: true
                                        visible: view.destNote(place.modelData) !== ""
                                        text: view.destNote(place.modelData)
                                        color: "#5d6d7e"
                                        font: Kirigami.Theme.smallFont
                                        wrapMode: Text.Wrap
                                    }
                                }
                                CardButton {
                                    visible: place.modelData.state === "not_mounted"
                                    enabled: !backend.busy
                                    text: qsTr("Mount")
                                    Accessible.name: qsTr("Mount %1").arg(view.destName(place.modelData))
                                    onClicked: backend.mount(place.modelData.device)
                                }
                                // Too small: shown greyed, the line above says why
                                CardButton {
                                    visible: place.usable || place.modelData.state === "too_small"
                                    enabled: place.usable
                                    checked: place.chosen
                                    text: place.chosen ? qsTr("Chosen") : qsTr("Choose")
                                    Accessible.name: text + " · " + view.destName(place.modelData)
                                    onClicked: view.dest = { kind: place.modelData.kind, path: place.modelData.path,
                                                             label: place.modelData.label || "" }
                                }
                            }
                        }
                    }
                }
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("To send the backup to your computer, install Syncthing in “More software”.")
                color: "#5d6d7e"
                font: Kirigami.Theme.smallFont
                wrapMode: Text.Wrap
            }
        }

        // ---- Step 3: when
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 3
            spacing: 10

            RowsBox {
                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 12
                    spacing: 10
                    TickBox {
                        id: scheduleBox
                        tickState: view.scheduleOn ? Qt.Checked : Qt.Unchecked
                        Accessible.name: qsTr("Back up every 2 hours")
                        onClicked: view.scheduleOn = !view.scheduleOn
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 1
                        Label {
                            text: qsTr("Back up every 2 hours")
                            color: "#1f2d3a"
                            font.bold: true
                            MouseArea {
                                anchors.fill: parent
                                onClicked: view.scheduleOn = !view.scheduleOn
                            }
                        }
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Skipped while KStars and INDI run a session, so imaging is never slowed down. Unticked: only when you press “Back up now”.")
                            color: "#5d6d7e"
                            font: Kirigami.Theme.smallFont
                            wrapMode: Text.Wrap
                        }
                    }
                }
                Separator {}
                GridLayout {
                    Layout.fillWidth: true
                    Layout.margins: 12
                    columns: view.narrow ? 1 : 2
                    columnSpacing: 8
                    rowSpacing: 8
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 1
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Versions of the settings to keep")
                            color: "#1f2d3a"
                            font.bold: true
                            wrapMode: Text.Wrap
                        }
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("One per backup; a version costs only what changed. Images are only added to: a file deleted here stays in the backup.")
                            color: "#5d6d7e"
                            font: Kirigami.Theme.smallFont
                            wrapMode: Text.Wrap
                        }
                    }
                    RowLayout {
                        spacing: 8
                        CardButton {
                            text: "−"
                            enabled: view.keep > 1
                            Accessible.name: qsTr("Fewer versions")
                            onClicked: view.keep = Math.max(1, view.keep - 1)
                        }
                        Label {
                            Layout.minimumWidth: 32
                            horizontalAlignment: Text.AlignHCenter
                            text: view.keep
                            color: "#1f2d3a"
                            font.bold: true
                            Accessible.name: qsTr("%n version(s)", "", view.keep)
                        }
                        CardButton {
                            text: "+"
                            enabled: view.keep < 100
                            Accessible.name: qsTr("More versions")
                            onClicked: view.keep = Math.min(100, view.keep + 1)
                        }
                    }
                }
                Separator { visible: !view.configured }
                RowLayout {
                    visible: !view.configured
                    Layout.fillWidth: true
                    Layout.margins: 12
                    spacing: 10
                    TickBox {
                        tickState: view.firstNow ? Qt.Checked : Qt.Unchecked
                        Accessible.name: qsTr("Make the first backup now")
                        onClicked: view.firstNow = !view.firstNow
                    }
                    Label {
                        Layout.fillWidth: true
                        text: qsTr("Make the first backup now")
                        color: "#1f2d3a"
                        font.bold: true
                        MouseArea {
                            anchors.fill: parent
                            onClicked: view.firstNow = !view.firstNow
                        }
                    }
                }
            }

            RowsBox {
                InfoRow {
                    label: qsTr("What")
                    value: qsTr("%n item(s) · %1", "", view.selectedCount()).arg(view.fmt(view.selectedBytes()))
                }
                Separator {}
                InfoRow {
                    label: qsTr("Where")
                    value: view.destName(view.dest)
                    sub: view.dest.path || ""
                }
            }
            Label {
                Layout.fillWidth: true
                visible: view.dest.kind === "internal" || view.dest.kind === "usb" || view.dest.kind === "syncthing"
                text: qsTr("The backup folder is created once with administrator rights: your password may be asked.")
                color: "#5d6d7e"
                font: Kirigami.Theme.smallFont
                wrapMode: Text.Wrap
            }
        }

        // The wizard's message and buttons, pinned under the page (Main.qml's navHost)
        ColumnLayout {
            id: wizardNavBar
            Layout.fillWidth: true
            spacing: 10
            visible: view.visible && view.navShown
            Component.onCompleted: if (view.navHost) parent = view.navHost
            ViewMessage {
                id: wizardMessage
            }
            // Wizard buttons
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                CardButton {
                    text: qsTr("Back")
                    visible: view.step > 1
                    onClicked: {
                        view.step -= 1
                        view.say("")
                    }
                }
                CardButton {
                    text: qsTr("Cancel")
                    onClicked: {
                        view.reload()
                        view.mode = "overview"
                        view.say("")
                    }
                }
                Item { Layout.fillWidth: true }
                CardButton {
                    highlighted: true
                    enabled: !backend.busy && (view.step !== 2 || view.destOk())
                    text: view.step < view.steps.length ? qsTr("Next")
                         : (view.configured ? qsTr("Save") : qsTr("Turn on the backup"))
                    onClicked: {
                        view.say("")
                        if (view.step === 1) {
                            view.step = 2
                            view.refreshDests()
                            // The chosen place if it is still there and usable, else this disk: ready at once
                            if (!view.destOk()) {
                                const internal = view.dests.find(d => d.kind === "internal" && d.state === "ready")
                                view.dest = internal ? { kind: internal.kind, path: internal.path, label: "" } : {}
                            }
                        } else if (view.step === 2) {
                            view.step = 3
                        } else {
                            view.apply()
                        }
                    }
                }
            }
        }
    }

    // ============================================================= SYSTEM
    // The backup tool's system backup (a btrfs image with snapshots, see its README):
    // where, how many to keep, the folders left out (the same list as the data backup,
    // with the backup tool's own folder list and protected folders), back up now
    property string sysDest: ""
    property int sysKeep: 5
    property string sysExcluded: ""         // absolute paths, one per line (backup tool format)
    property string sysState: ""            // "" | measuring | running | choose | folders
    property var sysMedia: []
    property var sysGroups: []
    property var sysSizes: ({})
    readonly property var sysExcludedMap: {
        const m = {}
        for (const p of view.sysExcluded.split("\n"))
            if (p !== "")
                m[p] = true
        return m
    }

    // The home's folders and the images, as rows of the list (locked: needed by AstroArch)
    function sysLoadFolders() {
        const home = system.home_path()
        const shared = system.shared_data_path()
        const row = (path, f) => ({ id: path, name: f.name, paths: [path.replace(home, "~")], exists: true,
                                    raw: true, locked: f.locked || !f.excludable, children: [] })
        const homeItems = JSON.parse(system.list_subfolders(home, true)).map(f => row(home + "/" + f.name, f))
        const groups = [{ id: "home", title: "Your home", items: homeItems }]
        if (shared !== "") {
            const children = JSON.parse(system.list_subfolders(shared, true))
                .filter(f => f.excludable)
                .map(f => ({ key: shared + "/" + f.name, name: f.name, paths: [shared + "/" + f.name], exists: true, note: "" }))
            groups.push({ id: "images", title: "Images and sessions", items: [
                { id: shared, name: "Images, sequences and targets", paths: [shared], exists: true,
                  subfolders: true, children: children } ] })
        }
        view.sysGroups = groups
        system.request_folder_sizes(home, true)
        if (shared !== "") {
            system.request_folder_sizes(shared, true)
            system.request_path_size(shared)
        }
    }

    // The drives for the system backup: not another system's disk (as the data backup)
    function sysListMedia() {
        view.sysMedia = JSON.parse(backend.offeredDrives(system.list_media()))
    }
    // Why a drive cannot hold the system backup, "" when it can
    function sysStateText(m) {
        return ({ fat32: qsTr("FAT32: cannot be used"),
                  too_small: qsTr("Not enough space: %1 free").arg(view.fmt(m.available)),
                  readonly: qsTr("Read-only"),
                  unsupported: qsTr("This format cannot be used") })[m.state] || ""
    }

    function sysDestName(path) {
        if (path === "" || path === "/var/backups")
            return qsTr("This disk")
        const m = view.sysMedia.find(x => x.destination === path)
        return m ? qsTr("USB drive %1").arg(m.label) : path
    }
    function sysStage(code) {
        return ({ preparing: qsTr("Preparing the backup file…"), copying: qsTr("Copying files…"),
                  boot: qsTr("Copying the boot partition…"), snapshot: qsTr("Creating the snapshot…"),
                  verifying: qsTr("Verifying the backup…"), pruning: qsTr("Removing old backups…"),
                  shrinking: qsTr("Shrinking the backup file…"), finishing: qsTr("Finishing…") })[code] || ""
    }
    function sysBackup() {
        view.sysState = "measuring"
        view.say(qsTr("Measuring what to back up… Your password is asked."))
        system.measure_data(view.sysExcluded)
    }

    BackupManager {
        id: system
        onConfigLoaded: (keep, excluded) => {
            view.sysKeep = keep
            view.sysExcluded = excluded
            view.sysDest = JSON.parse(backend.systemConfig()).dest || "/var/backups"
            view.sysListMedia()
        }
        onDataMeasured: (bytes, error) => {
            if (error !== "") {
                view.sysState = ""
                view.say(qsTr("Could not measure the data: %1").arg(error), true)
                return
            }
            const ev = JSON.parse(system.evaluate_destination(view.sysDest))
            if (!ev.enough) {
                view.sysState = "choose"
                view.sysListMedia()
                view.say(qsTr("Not enough space at %1 (%2 needed): choose another place.")
                         .arg(view.sysDestName(view.sysDest)).arg(view.fmt(ev.needed)), true)
                return
            }
            view.sysState = "running"
            view.say("")
            system.start_backup(view.sysDest, view.sysExcluded, view.sysKeep, true)
        }
        onBackupFinished: (success, error) => {
            view.sysState = ""
            view.say(success ? qsTr("System backup complete.") : qsTr("The system backup failed: %1").arg(error), !success)
        }
        onMediaMounted: (device, mountpoint, error) => {
            if (error !== "")
                view.say(qsTr("Could not mount the drive: %1").arg(error), true)
            view.sysListMedia()
        }
        onFolderSizeReady: (path, bytes) => {
            const s = Object.assign({}, view.sysSizes)
            s[path] = bytes
            view.sysSizes = s
        }
        onBackupCanceled: {
            view.sysState = ""
            view.say(qsTr("System backup cancelled. The previous backups are untouched."))
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        visible: view.profile === "system"
        spacing: 12

        RowsBox {
            InfoRow {
                label: qsTr("Where")
                value: view.sysDestName(view.sysDest)
                sub: view.sysDest
                CardButton {
                    text: qsTr("Change…")
                    enabled: view.sysState === ""
                    Accessible.name: qsTr("Change where the system backups go")
                    onClicked: {
                        view.sysListMedia()
                        view.sysState = "choose"
                    }
                }
            }
            Separator {}
            InfoRow {
                label: qsTr("Keep")
                value: qsTr("The last %n backup(s)", "", view.sysKeep)
                CardButton {
                    text: "−"
                    enabled: view.sysKeep > 1 && view.sysState === ""
                    Accessible.name: qsTr("Keep fewer system backups")
                    onClicked: view.sysKeep -= 1
                }
                CardButton {
                    text: "+"
                    enabled: view.sysKeep < 50 && view.sysState === ""
                    Accessible.name: qsTr("Keep more system backups")
                    onClicked: view.sysKeep += 1
                }
            }
            Separator {}
            InfoRow {
                label: qsTr("Left out")
                value: view.sysExcluded === "" ? qsTr("Nothing: the whole system and your home")
                     : qsTr("%n folder(s)", "", view.sysExcluded.split("\n").length)
                CardButton {
                    text: qsTr("Change…")
                    enabled: view.sysState === ""
                    Accessible.name: qsTr("Change the folders left out of the system backup")
                    onClicked: {
                        view.sysLoadFolders()
                        view.sysState = "folders"
                    }
                }
            }
            Separator {}
            InfoRow {
                label: qsTr("Before updating")
                value: qsTr("System backup offered")
                sub: qsTr("Ticked by default, it can be skipped")
            }
        }

        // Folders left out of the system backup: unticked = left out (kept empty)
        ColumnLayout {
            id: sysFolders
            Layout.fillWidth: true
            visible: view.sysState === "folders"
            spacing: 10
            Label {
                Layout.fillWidth: true
                text: qsTr("Untick what the system backup can leave out, such as images already kept elsewhere. Locked folders are needed by AstroArch.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            SelectionList {
                Layout.fillWidth: true
                groups: view.sysGroups
                excluded: view.sysExcludedMap
                sizes: view.sysSizes
                onEdited: (excluded) => view.sysExcluded = Object.keys(excluded).filter(k => excluded[k]).join("\n")
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                CardButton {
                    highlighted: true
                    text: qsTr("Done")
                    onClicked: {
                        const error = backend.saveSystemExcluded(view.sysExcluded)
                        view.say(error === "" ? "" : qsTr("Could not save the folders: %1").arg(error), error !== "")
                        view.sysState = ""
                    }
                }
            }
        }

        // Places for the system backup: the main disk, or a drive (not FAT32)
        RowsBox {
            id: sysPlaces
            visible: view.sysState === "choose"
            Repeater {
                model: [{ destination: "/var/backups", label: "", state: "ready", device: "" }].concat(view.sysMedia)
                ColumnLayout {
                    id: sysPlace
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true
                    spacing: 0
                    Separator { visible: sysPlace.index > 0 }
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.margins: 12
                        spacing: 10
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1
                            Label {
                                text: sysPlace.modelData.device === "" ? qsTr("This disk") : qsTr("USB drive %1").arg(sysPlace.modelData.label)
                                color: "#1f2d3a"
                                font.bold: true
                            }
                            Label {
                                Layout.fillWidth: true
                                text: sysPlace.modelData.destination || sysPlace.modelData.device
                                color: "#5d6d7e"
                                font.family: "monospace"
                                font.pointSize: Kirigami.Theme.smallFont.pointSize
                                elide: Text.ElideMiddle
                            }
                            Label {
                                Layout.fillWidth: true
                                visible: text !== ""
                                text: view.sysStateText(sysPlace.modelData)
                                color: "#8a4b00"
                                font: Kirigami.Theme.smallFont
                                wrapMode: Text.Wrap
                            }
                        }
                        CardButton {
                            visible: sysPlace.modelData.state === "not_mounted"
                            text: qsTr("Mount")
                            onClicked: system.mount_media(sysPlace.modelData.device)
                        }
                        // Too small, read-only...: greyed, the line on the left says why
                        CardButton {
                            visible: !!sysPlace.modelData.destination
                            enabled: sysPlace.modelData.state === "ready"
                            checked: view.sysDest === sysPlace.modelData.destination
                            text: view.sysDest === sysPlace.modelData.destination ? qsTr("Chosen") : qsTr("Choose")
                            onClicked: {
                                view.sysDest = sysPlace.modelData.destination
                                view.sysState = ""
                            }
                        }
                    }
                }
            }
            Separator {}
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 12
                Item { Layout.fillWidth: true }
                CardButton {
                    text: qsTr("Cancel")
                    onClicked: {
                        view.sysState = ""
                        view.say("")
                    }
                }
            }
        }

        // Progress of a running system backup
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.sysState === "running" || system.isRunning
            spacing: 6
            Label {
                text: view.sysStage(system.currentFile)
                color: "#1f2d3a"
            }
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 10
                radius: 5
                color: "#c9d3db"
                Accessible.role: Accessible.ProgressBar
                Accessible.name: qsTr("System backup progress")
                Rectangle {
                    width: parent.width * Math.max(0, Math.min(100, system.progressPercent)) / 100
                    height: parent.height
                    radius: 5
                    color: "#1d6fa5"
                }
            }
        }

        ViewMessage { id: systemMessage }
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Item { Layout.fillWidth: true }
            CardButton {
                visible: system.isRunning
                text: qsTr("Cancel")
                onClicked: system.cancel_backup()
            }
            CardButton {
                enabled: view.sysState === "" && !system.isRunning
                text: view.sysState === "measuring" ? qsTr("Measuring…") : qsTr("Back up the system now")
                onClicked: view.sysBackup()
            }
        }
    }
}

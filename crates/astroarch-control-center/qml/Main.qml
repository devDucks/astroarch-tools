import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import org.kde.kirigami as Kirigami
import AstroArchControlCenter

ApplicationWindow {
    id: root

    width: 940
    height: 660
    // Fits the official 800x480 Raspberry Pi touchscreen
    minimumWidth: 760
    minimumHeight: 460
    visible: true
    title: qsTr("AstroArch Control Center")
    color: Kirigami.Theme.backgroundColor

    // ------------------------------------------------------------------ pages
    // ONE list of pages. To add a page: add an entry here, nothing to change in Rust.
    //   setup:  "essential" | "recommended" | "optional" places it in the setup flow;
    //           "" = not a setup step
    //   topic:  where it goes when the setup is hidden
    //   detect: probe (scripts/astroarch-control-center-probe.sh) whose success means
    //           "already done on this system"; detected: what to say then
    //   hold:   probe whose success puts the step on hold
    //   rows:   label, value (fixed text) or probe (+ format, see formatValue), actions;
    //           choice: key of a per-row choice ("kept"). A page whose rows all have one
    //           is done once each row is changed on the system, kept, or not there for
    //           this user (see rowState); hideWhen: { probe, value } hides the row
    //   action: command (+ terminal: true; needs: probe that must succeed, the button is
    //           hidden when it is the row's own probe, as the row says why; when: values of
    //           the row's probe that show it, "on" or "off|missing"; tool: its name in
    //           messages when the label is not), or kind "keep" (stores the row's choice
    //           "kept") / "copy"
    //   view:   "software" shows the programs of the software installer in the page,
    //           "kiosk" the kiosk profiles and apps (KioskSetup.qml),
    //           "storage" moves the image folder to a drive and back,
    //           "services" the services of astroarch-sys-manager (ServicesView.qml)
    //   probes: probes the view reads, run with those of the rows
    readonly property var pages: [
        {
            id: "network", setup: "essential", topic: "system", detect: "network-ok",
            title: qsTr("Network and remote access"),
            intro: qsTr("How AstroArch is connected, and how to reach it from another device."),
            detected: "network",
            rows: [
                { label: qsTr("Home network"), probe: "network-home", missing: qsTr("Not connected"),
                  actions: [ { label: qsTr("Network settings"), command: "kcmshell6 kcm_networkmanagement" } ] },
                { label: qsTr("Field hotspot"), probe: "hotspot", missing: qsTr("Not set up"),
                  sub: qsTr("For the field, when there is no home network"),
                  actions: [ { label: qsTr("Hotspot settings"), command: "kcmshell6 kcm_networkmanagement", hotspot: true } ] },
                { label: qsTr("Reach it at"), probe: "host",
                  sub: qsTr("All the addresses are in “How to connect”"), actions: [] }
            ]
        },
        {
            id: "language", setup: "essential", topic: "system",
            title: qsTr("Language and time zone"),
            intro: qsTr("Check these three: they are what the system uses now."),
            primary: qsTr("These are right"),
            rows: [
                { label: qsTr("Language"), probe: "language", format: "locale",
                  actions: [ { label: qsTr("Change…"), command: "astroarch-locale-module --page language",
                               tool: qsTr("Language and keyboard") } ] },
                { label: qsTr("Keyboard"), probe: "keyboard", format: "keyboard",
                  actions: [ { label: qsTr("Change…"), command: "astroarch-locale-module --page keyboard",
                               tool: qsTr("Language and keyboard") } ] },
                { label: qsTr("Time zone"), probe: "timezone", format: "timezone",
                  actions: [ { label: qsTr("Change…"), command: "kcmshell6 kcm_clock", tool: qsTr("Date and time") } ] }
            ]
        },
        {
            id: "passwords", setup: "recommended", topic: "system",
            title: qsTr("Passwords"),
            intro: qsTr("AstroArch comes with default passwords. Change them, or keep them: both are fine choices."),
            // format "password": the probe says "default", "changed" or "none" (no such
            // password for this user); value = the default, missing = what "none" means
            rows: [
                { label: qsTr("User password"), choice: "password-user", probe: "password-user", format: "password",
                  value: qsTr("Default: astro"),
                  actions: [ { label: qsTr("Change…"), command: "kcmshell6 kcm_users", tool: qsTr("Users") },
                             { label: qsTr("Keep"), kind: "keep" } ] },
                { label: qsTr("Field Wi‑Fi"), choice: "password-wifi", probe: "password-hotspot", format: "password",
                  value: qsTr("Default: astronomy"), missing: qsTr("Not set up"),
                  sub: qsTr("On shared Wi‑Fi in the field, anyone nearby could connect"),
                  actions: [ { label: qsTr("Change…"), command: "kcmshell6 kcm_networkmanagement", hotspot: true,
                               tool: qsTr("Hotspot settings") },
                             { label: qsTr("Keep"), kind: "keep" } ] },
                // Separate from the user password: smbpasswd asks the old one, then the new one
                { label: qsTr("Shared folders"), choice: "password-samba", probe: "password-samba", format: "password",
                  value: qsTr("Default: astro"), missing: qsTr("Not set up for this user"),
                  sub: qsTr("To open your files from another computer"),
                  actions: [ { label: qsTr("Change…"), command: "smbpasswd", terminal: true, needs: "password-samba",
                               tool: qsTr("Shared folders password") },
                             { label: qsTr("Keep"), kind: "keep" } ] }
            ]
        },
        {
            id: "update", setup: "recommended", topic: "system", hold: "update-hold",
            title: qsTr("Update"),
            intro: qsTr("Bring AstroArch up to date. A system backup is offered first."),
            rows: [
                { label: qsTr("Installed"), probe: "version", format: "version",
                  actions: [ { label: qsTr("Update now"), command: "update-astroarch", terminal: true, needs: "update-command",
                               tool: qsTr("Update") } ] },
                { label: qsTr("Before updating"), value: qsTr("System backup offered"),
                  sub: qsTr("Ticked by default, it can be skipped"), actions: [] }
            ]
        },
        {
            id: "location", setup: "recommended", topic: "observing", detect: "location",
            title: qsTr("Location and time"),
            intro: qsTr("Where you observe from: KStars uses it for pointing and plate solving."),
            detected: qsTr("Found in KStars"),
            rows: [
                root.gpsRow,
                { label: qsTr("Observing site"), probe: "location", format: "location", missing: qsTr("Not set"),
                  sub: qsTr("Without a GPS: set it once in KStars, which saves it when it closes"),
                  actions: [ { label: qsTr("Set in KStars…"), command: "kstars", tool: "KStars" } ] }
            ]
        },
        {
            id: "equipment", setup: "recommended", topic: "observing", detect: "equipment-ok",
            title: qsTr("Equipment"),
            intro: qsTr("Your Ekos and PHD2 profiles: create them with their own wizards, or bring them from a backup."),
            detected: qsTr("Ekos and PHD2 profiles found"),
            rows: [
                { label: qsTr("Ekos profiles"), probe: "ekos-profiles", format: "profiles", missing: qsTr("None yet"),
                  sub: qsTr("In KStars: Ekos, then the profile wizard"),
                  actions: [ { label: qsTr("Open KStars"), command: "kstars", tool: "KStars" } ] },
                { label: qsTr("PHD2 profile"), probe: "phd2-profile", missing: qsTr("None with a camera and a mount"),
                  actions: [ { label: qsTr("Open PHD2"), command: "phd2", tool: "PHD2" } ] }
            ]
        },
        {
            id: "kiosk", setup: "optional", topic: "observing",
            title: qsTr("Kiosk"),
            intro: qsTr("A lighter second desktop for the field: it opens only the one or two imaging programs you use, and can reopen them if they close. Reach it by remote desktop on port 3390, next to the main desktop."),
            view: "kiosk",
            probes: ["kiosk-setup", "kiosk-profiles", "kiosk-apps", "kiosk-busy"],
            rows: []
        },
        {
            id: "storage", setup: "optional", topic: "data",
            title: qsTr("Image storage"),
            intro: qsTr("One folder shared by the main user and the kiosk user, so that both can save their images, sequences and targets in it."),
            // Moving it to a drive and back: StorageView.qml
            view: "storage",
            rows: [
                { label: qsTr("Folder"), probe: "astro-data", format: "folder", missing: qsTr("Not found"),
                  sub: qsTr("Also ~/astro-data in each home"), actions: [] },
                { label: qsTr("Kept on"), probe: "astro-data-place", format: "place", actions: [] },
                { label: qsTr("Space"), probe: "astro-data", format: "space", actions: [] }
            ]
        },
        {
            id: "software", setup: "optional", topic: "software",
            title: qsTr("More software"),
            intro: qsTr("Programs you can add. Installing or removing one asks for your password."),
            view: "software",
            rows: []
        },
        {
            id: "services", setup: "optional", topic: "software",
            title: qsTr("Services"),
            intro: qsTr("Devices and services you can switch on. Each one runs in a terminal, which may ask for your password."),
            // The state and the commands of astroarch-sys-manager (ServicesView.qml); the
            // GPS is on the Location page
            view: "services",
            probes: ["plugins-dir", "usb-current"],
            rows: []
        },
        {
            id: "monitor", setup: "", topic: "observing",
            title: qsTr("Astro monitor"),
            intro: qsTr("A companion for the night: it watches the imaging session and keeps a copy of your profiles. It runs in a terminal."),
            rows: [
                { label: qsTr("Session alerts"), value: qsTr("To Telegram, to your network, or both"),
                  sub: qsTr("Also without internet, on the local network"),
                  actions: [ { label: qsTr("Open Astro monitor"), command: "astromonitor", terminal: true, tool: "Astro monitor" } ] },
                { label: qsTr("Profile backup"), value: qsTr("KStars, INDI and PHD2 profiles and settings"),
                  sub: qsTr("On the AstroArch server, to restore them on any machine"), actions: [] }
            ]
        },
        {
            id: "summary", setup: "", topic: "connect",
            title: qsTr("How to connect"),
            intro: qsTr("The addresses to reach AstroArch from another device."),
            rows: [
                { label: qsTr("Remote desktop"), probe: "host", sub: qsTr("Kiosk desktop: port 3390"),
                  actions: [ { label: qsTr("Copy"), kind: "copy" } ] },
                { label: qsTr("Web browser"), probe: "host", format: "web",
                  actions: [ { label: qsTr("Copy"), kind: "copy" } ] },
                { label: qsTr("Your images"), probe: "host", format: "smb",
                  hideWhen: { probe: "password-samba", value: "none" },
                  actions: [ { label: qsTr("Copy"), kind: "copy" } ] }
            ]
        }
    ]

    // The GPS row of the Location page
    readonly property var gpsRow: ({
        label: qsTr("GPS"), probe: "gps", format: "service",
        sub: qsTr("Position and time from the GPS, when Ekos runs with its GPS driver"),
        actions: [ { label: qsTr("Turn on"), command: "gps_on", terminal: true, needs: "astroarch-shell",
                     when: "off|missing", tool: qsTr("GPS") },
                   { label: qsTr("Turn off"), command: "gps_off", terminal: true, needs: "astroarch-shell",
                     when: "on", tool: qsTr("GPS") } ]
    })

    readonly property var setupGroups: [
        { id: "essential", title: qsTr("Essentials", "setup group") },
        { id: "recommended", title: qsTr("Recommended", "setup group") },
        { id: "optional", title: qsTr("Optional", "setup group") },
        { id: "", title: qsTr("More", "setup group") }
    ]
    readonly property var topicGroups: [
        { id: "system", title: qsTr("System") },
        { id: "observing", title: qsTr("Observing") },
        { id: "data", title: qsTr("Data") },
        { id: "software", title: qsTr("Software and services") },
        { id: "connect", title: qsTr("Connection") }
    ]
    readonly property var levels: ({
        essential: { text: qsTr("Essential", "level of one page"), fg: "#8a1c16", bg: "#fbe7e5" },
        recommended: { text: qsTr("Recommended", "level of one page"), fg: "#0f5a8a", bg: "#e3f0f8" },
        optional: { text: qsTr("Optional", "level of one page"), fg: "#46525e", bg: "#e1e6ea" }
    })

    // ------------------------------------------------------------------ state
    property var probes: ({})           // probe name -> { ok, value }
    property var openGroups: ({})       // group key -> open/closed, as chosen by the user
    property string currentId: ""
    property string message: ""         // inline message on the current page
    property bool userPicked: false     // the user chose a page: probes no longer move it
    property var firstRunNames: []      // probes of the first run, when the window opens
    property bool firstRunDone: false
    property double lastRun: 0          // when the probes last ran (ms)
    property double lastHoldRun: 0      // the hold probe asks a server: at most every 5 min,
                                        // counted from its last answer
    property date now: new Date()       // for the local time next to the time zone

    // The view of the page shows its own primary button now (its property ownsPrimary, e.g.
    // the kiosk's "Apply"): the footer's is then not highlighted, and Enter alone does nothing
    readonly property bool viewOwnsPrimary: {
        const children = pageContent.children
        for (let i = 0; i < children.length; i++)
            if (children[i].visible && children[i].item && children[i].item.ownsPrimary === true)
                return true
        return false
    }

    readonly property var choices: {
        try {
            return JSON.parse(setup.choices || "{}")
        } catch (e) {
            return {}
        }
    }
    readonly property bool hidden: setup.hidden

    function pageById(id) {
        for (const p of root.pages)
            if (p.id === id)
                return p
        return root.pages[0]
    }

    function isTask(page) {
        return page.setup === "essential" || page.setup === "recommended"
    }

    function probeOk(name) {
        return root.probes[name] !== undefined && root.probes[name].ok
    }

    function onHold(page) {
        return !!page.hold && root.probeOk(page.hold)
    }

    // State of a row with a choice: "changed" or "none" (no such thing for this user) as
    // the system says, else "kept" as chosen, else "" (still to decide, or not read yet)
    function rowState(row) {
        const result = root.probes[row.probe]
        if (result !== undefined && result.ok && (result.value === "changed" || result.value === "none"))
            return result.value
        return root.choices[row.choice] === "kept" ? "kept" : ""
    }
    // Every row asks for a choice, and none is left to decide
    function rowsSettled(page) {
        return page.rows.length > 0 && page.rows.every(r => r.choice && root.rowState(r) !== "")
    }
    // Settled by keeping: defaults the system confirms were kept, nothing changed
    function keptDefaults(page) {
        const rows = page.rows.filter(r => r.choice)
        const keptDefault = r => root.rowState(r) === "kept" && root.probeOk(r.probe) && root.probes[r.probe].value === "default"
        return rows.some(keptDefault) && rows.every(r => keptDefault(r) || root.rowState(r) === "none")
    }
    function rowShown(row) {
        const when = row.hideWhen
        return !when || !root.probeOk(when.probe) || root.probes[when.probe].value !== when.value
    }

    // "done" | "later" | "skip" | "hold" | "" (to do). A hold only stops a step still to do
    function statusOf(page) {
        if ((page.detect && root.probeOk(page.detect)) || root.rowsSettled(page))
            return "done"
        const chosen = root.choices[page.id] || ""
        if (chosen === "" && root.onHold(page))
            return "hold"
        return chosen
    }

    readonly property var taskPages: root.pages.filter(p => root.isTask(p))
    readonly property int tasksDone: root.taskPages.filter(p => root.statusOf(p) === "done").length

    // Next page of the setup still to do, wrapping round; "summary" when none is left
    function nextOpen(fromId) {
        const flow = root.pages.filter(p => p.setup !== "")
        const start = flow.findIndex(p => p.id === fromId)
        for (let k = 1; k <= flow.length; k++) {
            const p = flow[(start + k + flow.length) % flow.length]
            if (root.isTask(p) && root.statusOf(p) === "")
                return p.id
        }
        return "summary"
    }

    // Where "Show setup" and the first start open: a page to do, else one set up later
    function resumePage() {
        const next = root.nextOpen("summary")
        if (next !== "summary")
            return next
        const later = root.taskPages.find(p => root.statusOf(p) === "later")
        return later ? later.id : "summary"
    }

    function choose(page, status) {
        setup.set_choice(page.id, status)
        root.go(root.nextOpen(page.id))
    }

    function go(id) {
        root.message = ""
        root.currentId = id
        const page = root.pageById(id)
        const open = Object.assign({}, root.openGroups)
        open[root.hidden ? "t-" + page.topic : "s-" + page.setup] = true
        root.openGroups = open
    }

    function runProbes() {
        const nowMs = Date.now()
        const names = new Set()
        const holdDue = nowMs - root.lastHoldRun > 5 * 60 * 1000
        for (const p of root.pages) {
            if (p.detect)
                names.add(p.detect)
            if (p.hold && (holdDue || root.probes[p.hold] === undefined))
                names.add(p.hold)
            for (const n of (p.probes || []))
                names.add(n)
            for (const r of p.rows) {
                if (r.probe)
                    names.add(r.probe)
                if (r.hideWhen)
                    names.add(r.hideWhen.probe)
                for (const a of r.actions)
                    if (a.needs)
                        names.add(a.needs)
            }
        }
        names.add("hotspot-uuid")
        root.lastRun = nowMs
        // The first page does not wait for the hold probe: it asks a server
        if (root.firstRunNames.length === 0)
            root.firstRunNames = Array.from(names).filter(n => !root.pages.some(p => p.hold === n))
        setup.run_probes(JSON.stringify(Array.from(names)))
    }

    function capitalized(text) {
        return text ? text.charAt(0).toUpperCase() + text.slice(1) : text
    }

    function formatValue(row) {
        if (!row.probe)
            return row.value || ""
        const result = root.probes[row.probe]
        if (result === undefined)
            return qsTr("Reading…")
        if (!result.ok)
            return row.format === "password" ? qsTr("Cannot be checked") : (row.missing || qsTr("Unknown"))
        const v = result.value
        switch (row.format) {
        case "locale": {
            const code = v.split(".")[0]
            const name = code === "C" || code === "POSIX" ? "" : Qt.locale(code).nativeLanguageName
            return name ? root.capitalized(name) : v
        }
        case "password":
            return v === "changed" ? qsTr("Changed")
                 : v === "none" ? (row.missing || qsTr("Not set up"))
                 : (root.choices[row.choice] === "kept" ? qsTr("Default kept") : row.value)
        case "keyboard":
            return v === "default" ? qsTr("System default") : v
        case "service":
            return ({ on: qsTr("On"), off: qsTr("Off"), missing: qsTr("Not installed") })[v] || v
        case "timezone": {
            // "Europe/Rome|+0100": the time of the zone, not of this process
            const parts = v.split("|")
            const offset = parseInt(parts[1])
            if (isNaN(offset))
                return parts[0]
            const minutes = Math.sign(offset) * (Math.floor(Math.abs(offset) / 100) * 60 + Math.abs(offset) % 100)
            const local = new Date(root.now.getTime() + (minutes + root.now.getTimezoneOffset()) * 60000)
            return parts[0] + " · " + qsTr("local time %1").arg(Qt.formatTime(local, "HH:mm"))
        }
        case "location": {
            const parts = v.split("|")
            const lat = parseFloat(parts[1]), lon = parseFloat(parts[2])
            let text = parts[0] || qsTr("Unnamed site")
            if (!isNaN(lat) && !isNaN(lon))
                text += " · " + Math.abs(lat).toFixed(2) + "° " + (lat >= 0 ? "N" : "S")
                        + ", " + Math.abs(lon).toFixed(2) + "° " + (lon >= 0 ? "E" : "W")
            return text
        }
        case "profiles": {
            const parts = v.split("|")
            return qsTr("%n profile(s) · e.g. %1", "", parseInt(parts[0])).arg(parts[1])
        }
        // The image folder: "path|used KiB|free KiB" and "internal" / "drive|name|type" /
        // "missing|name|type"
        case "folder":
            return v.split("|")[0]
        case "space": {
            const parts = v.split("|")
            const used = parseInt(parts[1]), free = parseInt(parts[2])
            if (root.probeOk("astro-data-place") && root.probes["astro-data-place"].value.startsWith("missing"))
                return qsTr("Unknown while the drive is not connected")
            const freeText = isNaN(free) ? "" : qsTr("%1 free").arg(Qt.locale().formattedDataSize(free * 1024))
            return isNaN(used) ? freeText
                 : [qsTr("%1 used").arg(Qt.locale().formattedDataSize(used * 1024)), freeText].filter(t => t).join(" · ")
        }
        case "place": {
            const parts = v.split("|")
            return parts[0] === "internal" ? qsTr("Internal disk")
                 : parts[0] === "drive" ? qsTr("Drive “%1” · %2").arg(parts[1]).arg(root.fsName(parts[2]))
                 : qsTr("Drive “%1” · not connected").arg(parts[1])
        }
        case "version":
            return "AstroArch " + v
        case "web":
            return v + ":8080"
        case "smb":
            return "smb://" + v + "/astro-data"
        }
        return v
    }

    // File system names as the drive's own tools show them
    function fsName(type) {
        return ({ exfat: "exFAT", vfat: "FAT32", ntfs: "NTFS", btrfs: "Btrfs", xfs: "XFS", f2fs: "F2FS" })[type] || type
    }

    // What the "detected" line of a page says
    function detectedText(page) {
        if (page.detected === "network" && root.probeOk("network-home"))
            return qsTr("Connected to %1").arg(root.probes["network-home"].value.split(" · ")[0])
        return page.detected === "network" ? qsTr("Connected to a network") : (page.detected || "")
    }

    function actionCommand(action) {
        // Network settings opened on the hotspot connection itself
        if (action.hotspot && root.probeOk("hotspot-uuid"))
            return action.command + " --args Uuid=" + root.probes["hotspot-uuid"].value
        return action.command
    }

    // "ok" | "checking" | "unavailable" (for this user) | "missing" (not installed)
    function actionState(action) {
        if (action.needs) {
            if (root.probes[action.needs] === undefined)
                return "checking"
            return root.probeOk(action.needs) ? "ok" : "unavailable"
        }
        if (action.command && !action.terminal)
            return launcher.available(action.command) ? "ok" : "missing"
        return "ok"
    }

    function runAction(page, row, action) {
        if (action.kind === "keep") {
            setup.set_choice(row.choice, "kept")
            return
        }
        if (action.kind === "copy") {
            clipboard.text = root.formatValue(row)
            clipboard.selectAll()
            clipboard.copy()
            root.message = qsTr("Copied %1.").arg(clipboard.text)
            return
        }
        const command = root.actionCommand(action)
        const result = launcher.launch(command, !!action.terminal)
        const name = action.tool || action.label.replace("…", "")
        if (result === "started") {
            root.message = qsTr("“%1” is open in its own window. When you have finished, come back here: this page updates by itself.").arg(name)
        } else if (result === "running") {
            root.message = qsTr("“%1” is already open.").arg(name)
        } else if (result === "missing") {
            root.message = qsTr("“%1” is not installed on this system.").arg(name)
        } else {
            root.message = qsTr("“%1” could not be started.").arg(name)
        }
    }

    // Hidden text field used to put text on the clipboard
    TextEdit {
        id: clipboard
        visible: false
    }

    Launcher {
        id: launcher
        // A tool was closed: what it changed is read again
        onFinished: (command) => {
            root.message = ""
            root.runProbes()
        }
    }

    SetupState {
        id: setup
        onProbed: (name, ok, value) => {
            const results = Object.assign({}, root.probes)
            results[name] = { ok: ok, value: value }
            root.probes = results
            if (root.pages.some(p => p.hold === name))
                root.lastHoldRun = Date.now()
            // Only while the window opens: if the first page turns out to be done or on
            // hold, go to the next one. Later the page stays put and shows what was found
            if (!root.firstRunDone && root.firstRunNames.every(n => root.probes[n] !== undefined)) {
                root.firstRunDone = true
                const page = root.pageById(root.currentId)
                const status = root.statusOf(page)
                if (!root.userPicked && !root.hidden && page.setup !== "" && (status === "done" || status === "hold"))
                    root.go(root.resumePage())
            }
        }
    }

    Component.onCompleted: {
        setup.load()
        root.runProbes()
        // --page ID opens on that page (a desktop launcher of one page, e.g. --page kiosk)
        const args = Qt.application.arguments
        const asked = args.indexOf("--page") >= 0 ? args[args.indexOf("--page") + 1] : ""
        if (root.pages.some(p => p.id === asked)) {
            root.userPicked = true
            root.go(asked)
        } else {
            root.currentId = setup.hidden ? root.pages[0].id : root.resumePage()
        }
    }

    // Back from a tool: read the system again, at most every few seconds
    onActiveChanged: if (active) reprobe.restart()
    Timer {
        id: reprobe
        interval: 1500
        onTriggered: if (Date.now() - root.lastRun > 4000) root.runProbes()
    }
    Timer {
        interval: 30000
        running: true
        repeat: true
        onTriggered: root.now = new Date()
    }

    // Enter = the focused text field's own Enter, else the focused button, else the primary
    // button of the page, unless its view shows its own (never "Apply" without a click);
    // Esc = "Set up later"
    Shortcut {
        sequences: ["Return", "Enter"]
        // Held down, it would go through several steps
        autoRepeat: false
        onActivated: {
            const focused = root.activeFocusItem
            // The shortcut gets Enter before a text field (TextField, TextInput): its
            // onAccepted runs, and nothing else
            if (focused && typeof focused.accepted === "function")
                focused.accepted()
            else if (focused && focused.visible && typeof focused.clicked === "function" && focused.enabled !== false)
                focused.clicked()
            else if (footer.visible && footer.primaryAction && !root.viewOwnsPrimary)
                footer.primaryAction()
        }
    }
    Shortcut {
        sequence: "Escape"
        autoRepeat: false
        onActivated: {
            const page = root.pageById(root.currentId)
            const status = root.statusOf(page)
            if (!root.hidden && root.isTask(page) && (status === "" || status === "hold"))
                root.choose(page, "later")
        }
    }

    // ---------------------------------------------------------------- layout

    // Stars of Sh2-308 by Matteo Beretta, behind the semi-transparent panels
    Image {
        anchors.fill: parent
        source: "qrc:/astroarch-control-center/images/sh2_308-only_stars.jpg"
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        opacity: 0.9
    }

    readonly property color panelColor: Qt.rgba(Kirigami.Theme.alternateBackgroundColor.r,
                                                Kirigami.Theme.alternateBackgroundColor.g,
                                                Kirigami.Theme.alternateBackgroundColor.b, 0.6)

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Header
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 76
            color: root.panelColor

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 20
                anchors.rightMargin: 20
                spacing: 12

                Image {
                    source: "qrc:/astroarch-control-center/images/astroarch-logo-white.svg"
                    sourceSize.width: 40
                    sourceSize.height: 40
                    fillMode: Image.PreserveAspectFit
                    Layout.preferredWidth: 40
                    Layout.maximumWidth: 40
                    Layout.preferredHeight: 40
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("AstroArch Control Center")
                        color: Kirigami.Theme.textColor
                        font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.9
                        font.weight: Font.Light
                        elide: Text.ElideRight
                    }
                    Label {
                        Layout.fillWidth: true
                        text: root.hidden
                              ? qsTr("Settings and tools of your AstroArch")
                              : qsTr("Setup: %1 of %2 done · you can stop at any time").arg(root.tasksDone).arg(root.taskPages.length)
                        color: Kirigami.Theme.disabledTextColor
                        elide: Text.ElideRight
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // Side panel: the groups open and close, the one of the current page is open
            Rectangle {
                Layout.preferredWidth: 240
                Layout.maximumWidth: 240
                Layout.fillHeight: true
                color: root.panelColor

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 4

                    ScrollView {
                        id: sideScroll
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        contentWidth: availableWidth
                        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                        ColumnLayout {
                            width: sideScroll.availableWidth
                            spacing: 2

                            Repeater {
                                model: root.hidden ? root.topicGroups : root.setupGroups

                                ColumnLayout {
                                    id: groupColumn

                                    required property var modelData
                                    readonly property string key: (root.hidden ? "t-" : "s-") + modelData.id
                                    readonly property var groupPages: root.pages.filter(p => root.hidden ? p.topic === modelData.id : p.setup === modelData.id)
                                    readonly property bool hasCurrent: groupPages.some(p => p.id === root.currentId)
                                    readonly property bool open: root.openGroups[key] !== undefined ? root.openGroups[key] : hasCurrent
                                    readonly property int doneCount: groupPages.filter(p => root.statusOf(p) === "done").length
                                    readonly property bool allDone: doneCount === groupPages.length
                                    readonly property string count: root.hidden ? ""
                                        : (modelData.id === "optional" ? qsTr("%1 · anytime").arg(groupPages.length)
                                        : (modelData.id === "" ? ""
                                        : (allDone ? qsTr("Done") : qsTr("%1 of %2").arg(doneCount).arg(groupPages.length))))

                                    Layout.fillWidth: true
                                    spacing: 1

                                    ItemDelegate {
                                        Layout.fillWidth: true
                                        text: groupColumn.modelData.title
                                        Accessible.name: groupColumn.modelData.title + (groupColumn.count ? ", " + groupColumn.count : "")
                                        Accessible.role: Accessible.Button
                                        Accessible.expandable: true
                                        Accessible.expanded: groupColumn.open
                                        onClicked: {
                                            const open = Object.assign({}, root.openGroups)
                                            open[groupColumn.key] = !groupColumn.open
                                            root.openGroups = open
                                        }

                                        contentItem: RowLayout {
                                            spacing: 8
                                            Kirigami.Icon {
                                                source: groupColumn.open ? "go-down" : "go-next"
                                                Layout.preferredWidth: 16
                                                Layout.preferredHeight: 16
                                            }
                                            Label {
                                                Layout.fillWidth: true
                                                text: groupColumn.modelData.title
                                                font.bold: true
                                                elide: Text.ElideRight
                                            }
                                            Label {
                                                text: groupColumn.count
                                                color: groupColumn.allDone ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                                            }
                                        }
                                    }

                                    Repeater {
                                        model: groupColumn.open ? groupColumn.groupPages : []

                                        ItemDelegate {
                                            id: pageDelegate

                                            required property var modelData
                                            readonly property bool current: modelData.id === root.currentId
                                            readonly property string status: root.hidden || modelData.setup === "" ? "none" : root.statusOf(modelData)
                                            readonly property string note: status === "later" ? qsTr("Set up later")
                                                : (status === "skip" ? qsTr("Not needed")
                                                : (status === "hold" ? qsTr("On hold")
                                                : (status === "done" && root.keptDefaults(modelData)
                                                   ? qsTr("Kept the defaults") : "")))

                                            Layout.fillWidth: true
                                            text: modelData.title
                                            Accessible.role: Accessible.Button
                                            Accessible.onPressAction: clicked()
                                            Accessible.name: modelData.title + ({ done: ", " + qsTr("done"), later: ", " + qsTr("set up later"),
                                                skip: ", " + qsTr("not needed"), hold: ", " + qsTr("on hold"), "": ", " + qsTr("to do"), none: "" })[status]
                                            onClicked: {
                                                root.userPicked = true
                                                root.go(modelData.id)
                                            }

                                            // Selected page: the system selection colour, as in Plasma's lists
                                            background: Rectangle {
                                                radius: 6
                                                color: pageDelegate.current
                                                       ? Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.3)
                                                       : (pageDelegate.hovered ? Qt.rgba(1, 1, 1, 0.07) : "transparent")
                                                border.width: pageDelegate.current || pageDelegate.visualFocus ? 1 : 0
                                                border.color: Kirigami.Theme.highlightColor
                                            }

                                            contentItem: RowLayout {
                                                spacing: 10
                                                Item {
                                                    Layout.preferredWidth: 16
                                                    Layout.preferredHeight: 16
                                                    Kirigami.Icon {
                                                        anchors.fill: parent
                                                        visible: ["done", "later", "hold"].includes(pageDelegate.status)
                                                        source: ({ done: "checkmark", later: "chronometer", hold: "media-playback-pause" })[pageDelegate.status] || ""
                                                        color: pageDelegate.status === "done" ? Kirigami.Theme.positiveTextColor
                                                               : (pageDelegate.status === "later" ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.disabledTextColor)
                                                        isMask: true
                                                    }
                                                    Rectangle {
                                                        anchors.centerIn: parent
                                                        visible: pageDelegate.status === "" || pageDelegate.status === "skip"
                                                        width: 12
                                                        height: 12
                                                        radius: 6
                                                        color: pageDelegate.current ? Kirigami.Theme.textColor : "transparent"
                                                        border.width: 2
                                                        border.color: pageDelegate.current ? Kirigami.Theme.textColor : Kirigami.Theme.disabledTextColor
                                                    }
                                                }
                                                ColumnLayout {
                                                    Layout.fillWidth: true
                                                    spacing: 0
                                                    Label {
                                                        Layout.fillWidth: true
                                                        text: pageDelegate.modelData.title
                                                        wrapMode: Text.Wrap
                                                    }
                                                    Label {
                                                        visible: pageDelegate.note !== "" && !pageDelegate.current
                                                        text: pageDelegate.note
                                                        color: Kirigami.Theme.disabledTextColor
                                                        font: Kirigami.Theme.smallFont
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Pinned at the bottom of the panel
                    ToolButton {
                        id: hideButton
                        Layout.alignment: Qt.AlignLeft
                        Layout.maximumWidth: parent.width
                        icon.name: root.hidden ? "view-list-details" : "view-hidden"
                        text: root.hidden ? qsTr("Show setup") : qsTr("I’m done, hide setup")
                        // A long translation goes on two lines instead of widening the panel
                        contentItem: RowLayout {
                            spacing: 6
                            Kirigami.Icon {
                                source: hideButton.icon.name
                                Layout.preferredWidth: 16
                                Layout.preferredHeight: 16
                            }
                            Label {
                                Layout.fillWidth: true
                                text: hideButton.text
                                wrapMode: Text.Wrap
                            }
                        }
                        onClicked: {
                            setup.hide_setup(!root.hidden)
                            hiddenNote.visible = setup.hidden
                            root.go(setup.hidden ? root.currentId : root.resumePage())
                        }
                    }
                    Label {
                        id: hiddenNote
                        Layout.fillWidth: true
                        leftPadding: 6
                        visible: false
                        text: qsTr("You can show the setup again from here.")
                        wrapMode: Text.Wrap
                        color: Kirigami.Theme.disabledTextColor
                        font: Kirigami.Theme.smallFont
                    }
                }
            }

            // Current page, on a light card as in the approved design
            Rectangle {
                id: card

                readonly property var page: root.pageById(root.currentId)
                readonly property string status: root.statusOf(page)
                readonly property var level: root.levels[page.setup]
                // Narrow window: each row puts its buttons under the value
                readonly property bool narrow: width < 620

                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.margins: 20
                radius: 8
                color: "#eff0f1"

                // Labels and messages of the card in Breeze Light colours
                Kirigami.Theme.colorSet: Kirigami.Theme.Window
                Kirigami.Theme.inherit: false
                Kirigami.Theme.backgroundColor: "#eff0f1"
                Kirigami.Theme.textColor: "#232629"

                // Every page starts at the top
                Connections {
                    target: root
                    function onCurrentIdChanged() {
                        pageScroll.contentItem.contentY = 0
                    }
                }

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 20
                    spacing: 12

                    ScrollView {
                        id: pageScroll
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        // Room for the scroll bar kept in any case: the text does not reflow when
                        // it appears, which would loop with its visibility
                        contentWidth: width - leftPadding - ScrollBar.vertical.width
                        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                        // Always visible when the page does not fit: the window may be small
                        ScrollBar.vertical.policy: contentHeight > availableHeight ? ScrollBar.AlwaysOn : ScrollBar.AsNeeded

                        ColumnLayout {
                            id: pageContent
                            width: pageScroll.contentWidth
                            spacing: 14

                            Accessible.role: Accessible.Pane
                            Accessible.name: card.page.title

                            Flow {
                                Layout.fillWidth: true
                                spacing: 10
                                Label {
                                    text: card.page.title
                                    color: "#1f2d3a"
                                    font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.35
                                    font.bold: true
                                }
                                Label {
                                    visible: !root.hidden && card.level !== undefined
                                    text: card.level ? card.level.text : ""
                                    color: card.level ? card.level.fg : "transparent"
                                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                                    font.bold: true
                                    font.capitalization: Font.AllUppercase
                                    font.letterSpacing: 0.5
                                    leftPadding: 8
                                    rightPadding: 8
                                    topPadding: 3
                                    bottomPadding: 3
                                    background: Rectangle {
                                        radius: height / 2
                                        color: card.level ? card.level.bg : "transparent"
                                    }
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                text: card.page.intro
                                color: "#34495e"
                                wrapMode: Text.Wrap
                            }

                            Kirigami.InlineMessage {
                                Layout.fillWidth: true
                                visible: root.onHold(card.page)
                                type: Kirigami.MessageType.Warning
                                text: root.onHold(card.page)
                                      ? qsTr("On hold: the AstroArch team asks not to update for now (“%1”). This step unlocks by itself when the notice is withdrawn.").arg(root.probes[card.page.hold].value)
                                      : ""
                            }

                            RowLayout {
                                visible: !!card.page.detect && root.probeOk(card.page.detect)
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
                                    text: root.detectedText(card.page)
                                    color: "#1e7a45"
                                    wrapMode: Text.Wrap
                                }
                            }

                            // Current values, one per row, with their actions
                            Rectangle {
                                visible: card.page.rows.length > 0
                                Layout.fillWidth: true
                                implicitHeight: rowsColumn.implicitHeight
                                radius: 8
                                color: "#ffffff"
                                border.color: "#d3dbe2"

                                ColumnLayout {
                                    id: rowsColumn
                                    width: parent.width
                                    spacing: 0

                                    Repeater {
                                        model: card.page.rows

                                        ColumnLayout {
                                            id: valueRow

                                            required property var modelData
                                            required property int index
                                            readonly property string chosen: modelData.choice ? (root.choices[modelData.choice] || "") : ""
                                            readonly property string rowState: modelData.choice ? root.rowState(modelData) : ""

                                            // Hidden, not filtered out: a new list at each probe would
                                            // rebuild the rows and lose the keyboard focus
                                            visible: root.rowShown(modelData)
                                            Layout.fillWidth: true
                                            spacing: 0

                                            Rectangle {
                                                visible: valueRow.index > 0
                                                Layout.fillWidth: true
                                                implicitHeight: 1
                                                color: "#e1e6ea"
                                            }

                                            GridLayout {
                                                Layout.fillWidth: true
                                                Layout.margins: 12
                                                columns: card.narrow ? 1 : 3
                                                columnSpacing: 16
                                                rowSpacing: 8

                                                Label {
                                                    Layout.preferredWidth: card.narrow ? -1 : 130
                                                    Layout.fillWidth: card.narrow
                                                    text: valueRow.modelData.label
                                                    color: "#4f5a64"
                                                    font.bold: true
                                                    wrapMode: Text.Wrap
                                                }
                                                ColumnLayout {
                                                    Layout.fillWidth: true
                                                    spacing: 2
                                                    Label {
                                                        Layout.fillWidth: true
                                                        text: root.formatValue(valueRow.modelData)
                                                        color: "#1f2d3a"
                                                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                                                    }
                                                    Label {
                                                        Layout.fillWidth: true
                                                        visible: !!valueRow.modelData.sub
                                                        text: valueRow.modelData.sub || ""
                                                        color: "#5d6d7e"
                                                        font: Kirigami.Theme.smallFont
                                                        wrapMode: Text.Wrap
                                                    }
                                                }
                                                RowLayout {
                                                    spacing: 8
                                                    visible: valueRow.modelData.actions.length > 0

                                                    Repeater {
                                                        model: valueRow.modelData.actions

                                                        CardButton {
                                                            required property var modelData
                                                            readonly property string availability: root.actionState(modelData)
                                                            readonly property bool held: root.onHold(card.page)
                                                            readonly property bool isKept: modelData.kind === "keep" && valueRow.chosen === "kept"

                                                            // Nothing to do on a row the user does not have; nothing to keep
                                                            // once the system says it was changed
                                                            visible: valueRow.rowState !== "none" && !(modelData.kind === "keep" && valueRow.rowState === "changed")
                                                                     && !(modelData.needs === valueRow.modelData.probe && availability === "unavailable")
                                                                     && (!modelData.when || (root.probeOk(valueRow.modelData.probe)
                                                                         && modelData.when.split("|").includes(root.probes[valueRow.modelData.probe].value)))
                                                            text: isKept ? qsTr("Kept")
                                                                 : availability === "missing" ? qsTr("%1 (not installed)").arg(modelData.label)
                                                                 : availability === "unavailable" ? qsTr("%1 (not available for this user)").arg(modelData.label)
                                                                 : availability === "checking" ? qsTr("%1 (checking…)").arg(modelData.label)
                                                                 : modelData.label
                                                            enabled: availability === "ok" && !held && !isKept
                                                            // "Change…" alone is ambiguous with several rows
                                                            Accessible.name: text + " · " + valueRow.modelData.label
                                                            onClicked: root.runAction(card.page, valueRow.modelData, modelData)
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Kept once opened: an install goes on while another page is shown
                            Loader {
                                id: softwareList
                                Layout.fillWidth: true
                                visible: card.page.view === "software"
                                active: false
                                onVisibleChanged: if (visible) active = true
                                Component.onCompleted: if (visible) active = true
                                sourceComponent: SoftwareList {}
                            }
                            // The services: their state is polled while the page is open
                            Loader {
                                Layout.fillWidth: true
                                visible: card.page.view === "services"
                                active: visible
                                sourceComponent: ServicesView {
                                    probes: root.probes
                                }
                            }
                            // Kept once opened too: a copy goes on while another page is shown
                            Loader {
                                Layout.fillWidth: true
                                visible: card.page.view === "storage"
                                active: false
                                onVisibleChanged: if (visible) active = true
                                Component.onCompleted: if (visible) active = true
                                sourceComponent: StorageView {
                                    probes: root.probes
                                    fsName: root.fsName
                                    onSystemChanged: root.runProbes()
                                }
                            }

                            // Kept once opened, like the software list: a change being applied
                            // still reports back when another page is shown
                            Loader {
                                id: kioskView
                                Layout.fillWidth: true
                                visible: card.page.view === "kiosk"
                                active: false
                                onVisibleChanged: if (visible) active = true
                                Component.onCompleted: if (visible) active = true
                                sourceComponent: KioskSetup {
                                    probes: root.probes
                                    toolLauncher: launcher
                                    setupState: setup
                                }
                            }

                            Kirigami.InlineMessage {
                                Layout.fillWidth: true
                                visible: root.message !== ""
                                type: Kirigami.MessageType.Information
                                text: root.message
                            }
                        }
                    }

                    // Footer: the setup actions of the page, always visible
                    RowLayout {
                        id: footer

                        readonly property bool setupPage: !root.hidden && card.page.setup !== ""
                        // What Enter does on this page when no button has the focus
                        readonly property var primaryAction: {
                            if (card.page.id === "summary")
                                return root.hidden ? null : () => { setup.hide_setup(true); hiddenNote.visible = true }
                            if (!setupPage || card.status === "hold")
                                return null
                            if (card.status === "done")
                                return () => root.go(root.nextOpen(card.page.id))
                            return () => root.choose(card.page, "done")
                        }

                        Layout.fillWidth: true
                        spacing: 10
                        visible: primaryAction !== null || (setupPage && card.status !== "done")

                        Item {
                            Layout.fillWidth: true
                        }
                        CardButton {
                            visible: footer.setupPage && card.status !== "done" && card.status !== "later" && card.page.setup !== "optional"
                            text: qsTr("Set up later")
                            onClicked: root.choose(card.page, "later")
                        }
                        CardButton {
                            visible: footer.setupPage && card.status !== "done" && card.status !== "skip" && card.page.setup === "optional"
                            text: qsTr("I don’t need this")
                            onClicked: root.choose(card.page, "skip")
                        }
                        CardButton {
                            visible: footer.primaryAction !== null
                            highlighted: !root.viewOwnsPrimary
                            text: card.page.id === "summary" ? qsTr("Finish")
                                  : (card.status === "done" ? qsTr("Next step")
                                  : (card.page.setup === "optional" ? qsTr("Done") : (card.page.primary || qsTr("Continue"))))
                            onClicked: footer.primaryAction()
                        }
                    }
                }
            }
        }
    }
}

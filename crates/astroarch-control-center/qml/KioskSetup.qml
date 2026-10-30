import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The Kiosk page, in the card: profiles shared with the main desktop or separate, and
// the apps the kiosk opens. The system is the source of truth: the probes kiosk-profiles
// and kiosk-apps run "astroarch-kiosk-setup status" (scripts/ of the astroarch
// repository). "Apply" runs "astroarch-kiosk-setup apply" through pkexec (the command
// is the value of the probe kiosk-setup), then the page reads the system again and says
// whether it changed as asked
ColumnLayout {
    id: kiosk

    property var probes: ({})
    property var toolLauncher
    property var setupState

    // ------------------------------------------------------------------ the system
    function ok(name) {
        return kiosk.probes[name] !== undefined && kiosk.probes[name].ok
    }
    readonly property bool read: kiosk.probes["kiosk-profiles"] !== undefined && kiosk.probes["kiosk-apps"] !== undefined
    readonly property bool found: kiosk.ok("kiosk-profiles")
    readonly property bool canApply: kiosk.ok("kiosk-setup")
    // "shared", "separate", "separate own" (the kiosk has profiles of its own), "mixed"
    readonly property string sysProfiles: kiosk.found ? kiosk.probes["kiosk-profiles"].value : ""
    readonly property bool sysShared: kiosk.sysProfiles === "shared"
    readonly property bool sysSeparate: kiosk.sysProfiles.startsWith("separate")
    readonly property var sysApps: {
        try {
            return kiosk.ok("kiosk-apps") ? JSON.parse(kiosk.probes["kiosk-apps"].value) : []
        } catch (e) {
            return []
        }
    }
    // KStars or PHD2 open in the kiosk: the profiles cannot change meanwhile
    readonly property string busy: kiosk.ok("kiosk-busy") ? kiosk.probes["kiosk-busy"].value : ""

    // --------------------------------------------------- choices not applied yet
    property string chosenProfiles: ""      // "" = as the system is
    property var chosenApps: ({})           // id -> { desktop, menu, start, watch }
    readonly property string profiles: kiosk.chosenProfiles || (kiosk.sysSeparate ? "separate" : "shared")
    readonly property bool profilesChanged: kiosk.found && (kiosk.profiles === "shared" ? !kiosk.sysShared : !kiosk.sysSeparate)
    readonly property bool appsChanged: Object.keys(kiosk.chosenApps).length > 0
    readonly property bool changed: kiosk.profilesChanged || kiosk.appsChanged
    // "Apply" is shown: the page's primary button (Main.qml)
    readonly property bool ownsPrimary: kiosk.changed

    readonly property var optionKeys: ["desktop", "menu", "start", "watch"]
    readonly property int cellWidth: kiosk.width < 560 ? 64 : 96
    // The app ids, changed only when the list itself changes: the rows (and the keyboard
    // focus) stay when the probes come back
    property var appIds: []
    function updateAppIds() {
        const ids = kiosk.sysApps.map(a => a.id)
        if (ids.join() !== kiosk.appIds.join())
            kiosk.appIds = ids
    }
    onSysAppsChanged: kiosk.updateAppIds()
    Component.onCompleted: kiosk.updateAppIds()
    readonly property var optionTexts: [qsTr("Icon on the desktop"), qsTr("In the menu"),
                                        qsTr("Opens with the kiosk"), qsTr("Reopens if closed")]

    property bool working: false
    property var waiting: []                // probes still to come back after "Apply"
    property bool movedAside: false         // the kiosk's own profiles were moved aside
    property bool profilesAsked: false      // "Apply" changed the profiles (also from "mixed")
    property string message: ""
    property bool failed: false

    function optionOn(app, key) {
        const chosen = kiosk.chosenApps[app.id]
        return chosen ? chosen[key] : app[key]
    }

    function sameAsSystem(app, options) {
        return kiosk.optionKeys.every(k => options[k] === app[k])
    }

    // Reopening an app needs it started with the kiosk, as the helper does
    function setOption(app, key, on) {
        const options = {}
        for (const k of kiosk.optionKeys)
            options[k] = kiosk.optionOn(app, k)
        options[key] = on
        if (key === "watch" && on)
            options.start = true
        if (key === "start" && !on)
            options.watch = false
        const all = Object.assign({}, kiosk.chosenApps)
        if (kiosk.sameAsSystem(app, options))
            delete all[app.id]
        else
            all[app.id] = options
        kiosk.chosenApps = all
        kiosk.message = ""
    }

    function pickProfiles(mode, radio) {
        const asSystem = mode === "shared" ? kiosk.sysShared : kiosk.sysSeparate
        kiosk.chosenProfiles = asSystem ? "" : mode
        kiosk.message = ""
        radio.checked = Qt.binding(() => kiosk.profiles === mode)
    }

    function undo() {
        kiosk.chosenProfiles = ""
        kiosk.chosenApps = {}
        kiosk.message = ""
    }

    function apply() {
        const args = []
        if (kiosk.profilesChanged)
            args.push("--profiles", kiosk.profiles)
        for (const id in kiosk.chosenApps) {
            const options = kiosk.chosenApps[id]
            args.push("--app", id + "=" + kiosk.optionKeys.filter(k => options[k]).join(","))
        }
        kiosk.profilesAsked = kiosk.profilesChanged
        kiosk.movedAside = kiosk.profilesChanged && kiosk.profiles === "shared" && kiosk.sysProfiles === "separate own"
        const result = kiosk.toolLauncher.launch(kiosk.probes["kiosk-setup"].value + " apply " + args.join(" "), false)
        kiosk.failed = result !== "started"
        if (result === "started") {
            kiosk.working = true
            kiosk.message = ""
        } else if (result === "running") {
            kiosk.message = qsTr("The kiosk is already being changed.")
        } else {
            kiosk.message = qsTr("The kiosk could not be changed: its setup program could not be started.")
        }
    }

    // Read back from the system: did it change as asked?
    function checkApplied() {
        kiosk.working = false
        const profilesDone = !kiosk.profilesChanged
        const appsDone = Object.keys(kiosk.chosenApps).every(id => {
            const app = kiosk.sysApps.find(a => a.id === id)
            return app && kiosk.sameAsSystem(app, kiosk.chosenApps[id])
        })
        const startsLater = Object.values(kiosk.chosenApps).some(o => o.start)
        if (profilesDone && appsDone) {
            const said = []
            if (kiosk.profilesAsked)
                said.push(kiosk.sysShared
                          ? (kiosk.movedAside
                             ? qsTr("The kiosk now uses the profiles of the main desktop. Its own profiles were moved aside, in its folder ~/.local/share/astroarch.")
                             : qsTr("The kiosk now uses the profiles of the main desktop."))
                          : qsTr("The kiosk now has profiles of its own."))
            if (kiosk.appsChanged)
                said.push(startsLater ? qsTr("The apps are set: those that open with the kiosk open at its next start.")
                                      : qsTr("The apps are set."))
            kiosk.chosenProfiles = ""
            kiosk.chosenApps = {}
            kiosk.failed = false
            kiosk.message = qsTr("Done.") + " " + said.join(" ")
        } else if (!profilesDone && kiosk.busy !== "") {
            // The helper changes nothing while they are open: the warning above says it
            kiosk.failed = true
            kiosk.message = qsTr("Nothing was changed.")
        } else {
            kiosk.failed = true
            kiosk.message = qsTr("The kiosk did not change as asked: the password was refused, or a change failed. The page shows what the system has now.")
        }
    }

    Connections {
        target: kiosk.toolLauncher
        function onFinished(command) {
            // The Control Center reads every probe again when a command ends
            if (kiosk.working && command.startsWith(kiosk.probes["kiosk-setup"].value + " apply"))
                kiosk.waiting = ["kiosk-profiles", "kiosk-apps", "kiosk-busy"]
        }
    }
    Connections {
        target: kiosk.setupState
        function onProbed(name, ok, value) {
            if (!kiosk.waiting.includes(name))
                return
            kiosk.waiting = kiosk.waiting.filter(n => n !== name)
            // The probes of the page are read again after this signal: wait for them
            if (kiosk.waiting.length === 0)
                Qt.callLater(kiosk.checkApplied)
        }
    }

    spacing: 14

    // ------------------------------------------------------------ light controls
    // Drawn here like CardButton: the Plasma style would draw them with the dark theme
    component CardRadio: RadioButton {
        id: radio
        indicator: Rectangle {
            x: radio.leftPadding
            y: radio.topPadding + 2
            implicitWidth: 20
            implicitHeight: 20
            radius: 10
            color: "#ffffff"
            border.width: 2
            border.color: radio.visualFocus ? "#3daee9" : (radio.checked ? "#1d6fa5" : "#7d8994")
            Rectangle {
                anchors.centerIn: parent
                width: 10
                height: 10
                radius: 5
                color: "#1d6fa5"
                visible: radio.checked
            }
        }
    }
    component CardCheck: CheckBox {
        id: check
        spacing: 8
        indicator: Rectangle {
            x: check.leftPadding
            y: check.topPadding + (check.availableHeight - height) / 2
            implicitWidth: 20
            implicitHeight: 20
            radius: 3
            color: check.checked ? (check.enabled ? "#1d6fa5" : "#9db7cb") : "#ffffff"
            border.width: check.visualFocus ? 2 : 1
            border.color: check.visualFocus ? "#3daee9"
                        : (!check.enabled ? "#c9ced3" : (check.checked ? "#1d6fa5" : "#7d8994"))
            Kirigami.Icon {
                anchors.centerIn: parent
                width: 14
                height: 14
                source: "checkmark"
                isMask: true
                color: "#ffffff"
                visible: check.checked
            }
        }
        contentItem: Label {
            leftPadding: check.text ? check.indicator.width + check.spacing : check.indicator.width
            text: check.text
            color: check.enabled ? "#1f2d3a" : "#7a828a"
            verticalAlignment: Text.AlignVCenter
            wrapMode: Text.Wrap
        }
    }
    component Mono: Label {
        color: "#5d6d7e"
        font.family: "Noto Sans Mono"
        font.pointSize: Kirigami.Theme.smallFont.pointSize
        wrapMode: Text.WrapAnywhere
    }

    // ------------------------------------------------------------------ messages
    Label {
        visible: !kiosk.read
        text: qsTr("Reading…")
        color: "#5d6d7e"
    }
    Kirigami.InlineMessage {
        Layout.fillWidth: true
        // Without the setup program, what can still be read is shown, without changes
        visible: kiosk.read && kiosk.probes["kiosk-setup"] !== undefined && (!kiosk.canApply || !kiosk.found)
        type: Kirigami.MessageType.Warning
        text: !kiosk.canApply ? qsTr("The kiosk cannot be set up from here: its setup program is missing. Update AstroArch.")
                              : qsTr("No kiosk was found on this system.")
    }

    // ------------------------------------------------------------------ profiles
    ColumnLayout {
        visible: kiosk.found
        Layout.fillWidth: true
        spacing: 8

        Label {
            text: qsTr("Profiles")
            color: "#4f5a64"
            font.bold: true
        }
        Label {
            Layout.fillWidth: true
            text: kiosk.sysProfiles === "mixed"
                  ? qsTr("Now: partly shared. Apply a choice to finish it.")
                  : qsTr("Whether the kiosk uses the same KStars, INDI and PHD2 profiles as the main desktop.")
            color: "#34495e"
            wrapMode: Text.Wrap
        }

        // Shared
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: sharedColumn.implicitHeight + 28
            radius: 8
            color: "#ffffff"
            border.width: 2
            border.color: kiosk.profiles === "shared" ? "#1d6fa5" : "#c3ccd4"

            ColumnLayout {
                id: sharedColumn
                x: 14
                y: 14
                width: parent.width - 28
                spacing: 8

                CardRadio {
                    id: sharedRadio
                    Layout.fillWidth: true
                    checked: kiosk.profiles === "shared"
                    enabled: !kiosk.working && kiosk.canApply
                    text: qsTr("Use the same profiles as the main desktop")
                    onToggled: kiosk.pickProfiles("shared", this)
                    contentItem: Flow {
                        leftPadding: sharedRadio.indicator.width + 10
                        spacing: 10
                        Label {
                            text: sharedRadio.text
                            color: "#1f2d3a"
                            font.bold: true
                        }
                        Label {
                            text: qsTr("Recommended", "level of one choice")
                            color: "#0f5a8a"
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                            font.bold: true
                            font.capitalization: Font.AllUppercase
                            leftPadding: 8
                            rightPadding: 8
                            topPadding: 2
                            bottomPadding: 2
                            background: Rectangle {
                                radius: height / 2
                                color: "#e3f0f8"
                            }
                        }
                    }
                }
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 30
                    text: qsTr("A profile, a piece of equipment or a dark frame made on one desktop is ready on the other.")
                    color: "#5d6d7e"
                    wrapMode: Text.Wrap
                }
                GridLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 30
                    columns: kiosk.width < 560 ? 1 : 2
                    columnSpacing: 20
                    rowSpacing: 6
                    Repeater {
                        model: [
                            { text: qsTr("KStars profiles, equipment and dark library"), path: "~/.local/share/kstars" },
                            { text: qsTr("KStars settings"), path: "~/.config/kstarsrc" },
                            { text: qsTr("INDI driver settings"), path: "~/.indi" },
                            { text: qsTr("PHD2 profiles and dark library"), path: "~/.PHDGuidingV2, ~/.phd2" }
                        ]
                        ColumnLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            spacing: 0
                            Label {
                                Layout.fillWidth: true
                                text: parent.modelData.text
                                color: "#34495e"
                                wrapMode: Text.Wrap
                            }
                            Mono {
                                Layout.fillWidth: true
                                text: parent.modelData.path
                            }
                        }
                    }
                }
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 30
                    text: qsTr("Plate-solving index files are shared too. Use KStars and PHD2 on one desktop at a time: the kiosk does not open them while they are open on the main desktop.")
                    color: "#5d6d7e"
                    font: Kirigami.Theme.smallFont
                    wrapMode: Text.Wrap
                }
                Kirigami.InlineMessage {
                    Layout.fillWidth: true
                    Layout.leftMargin: 30
                    visible: kiosk.profiles === "shared" && kiosk.sysProfiles === "separate own"
                    type: Kirigami.MessageType.Warning
                    text: qsTr("The kiosk already has profiles of its own. They will be moved aside, not deleted, in its folder ~/.local/share/astroarch. You can bring them back by choosing separate profiles again.")
                }
            }
        }

        // Separate
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: separateColumn.implicitHeight + 28
            radius: 8
            color: "#ffffff"
            border.width: 2
            border.color: kiosk.profiles === "separate" ? "#1d6fa5" : "#c3ccd4"

            ColumnLayout {
                id: separateColumn
                x: 14
                y: 14
                width: parent.width - 28
                spacing: 8

                CardRadio {
                    id: separateRadio
                    Layout.fillWidth: true
                    checked: kiosk.profiles === "separate"
                    enabled: !kiosk.working && kiosk.canApply
                    text: qsTr("Keep separate profiles for the kiosk")
                    onToggled: kiosk.pickProfiles("separate", this)
                    contentItem: Label {
                        leftPadding: separateRadio.indicator.width + 10
                        text: separateRadio.text
                        color: "#1f2d3a"
                        font.bold: true
                        wrapMode: Text.Wrap
                    }
                }
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 30
                    text: qsTr("For a second set of equipment, or another person using the kiosk. Its equipment is set up from the kiosk.")
                    color: "#5d6d7e"
                    wrapMode: Text.Wrap
                }
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 30
                    visible: kiosk.profiles === "separate" && !kiosk.sysSeparate
                    text: qsTr("The kiosk gets back the profiles it had before sharing, if any; otherwise it starts with none. Plate solving keeps the same index files.")
                    color: "#5d6d7e"
                    font: Kirigami.Theme.smallFont
                    wrapMode: Text.Wrap
                }
            }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: kiosk.profilesChanged && kiosk.busy !== ""
            type: Kirigami.MessageType.Warning
            text: qsTr("Close %1 in the kiosk first: the profiles cannot change while KStars or PHD2 is open there.").arg(kiosk.busy)
        }
    }

    // ---------------------------------------------------------------------- apps
    ColumnLayout {
        visible: kiosk.found && kiosk.sysApps.length > 0
        Layout.fillWidth: true
        spacing: 8

        Label {
            text: qsTr("Apps")
            color: "#4f5a64"
            font.bold: true
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("What the kiosk shows and opens. Icons and menu change at once; an app set to open with the kiosk opens at its next start, not now.")
            color: "#34495e"
            wrapMode: Text.Wrap
        }

        // One row per app, one column per option (same widths in the header and the rows)
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: appsColumn.implicitHeight
            radius: 8
            color: "#ffffff"
            border.color: "#d3dbe2"

            ColumnLayout {
                id: appsColumn
                width: parent.width
                spacing: 0

                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 12
                    Layout.bottomMargin: 4
                    spacing: 8
                    Item {
                        Layout.fillWidth: true
                    }
                    Repeater {
                        model: kiosk.optionTexts
                        Label {
                            required property string modelData
                            Layout.preferredWidth: kiosk.cellWidth
                            Layout.alignment: Qt.AlignBottom
                            text: modelData
                            color: "#4f5a64"
                            font: Kirigami.Theme.smallFont
                            horizontalAlignment: Text.AlignHCenter
                            wrapMode: Text.Wrap
                        }
                    }
                }

                Repeater {
                    model: kiosk.appIds

                    ColumnLayout {
                        id: appRow

                        required property string modelData
                        readonly property var app: kiosk.sysApps.find(a => a.id === modelData) || ({})
                        readonly property bool usable: !!app.installed && kiosk.canApply && !kiosk.working

                        Layout.fillWidth: true
                        spacing: 0

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 1
                            color: "#e1e6ea"
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            Layout.leftMargin: 12
                            Layout.rightMargin: 12
                            Layout.topMargin: 6
                            Layout.bottomMargin: 6
                            spacing: 8

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label {
                                    Layout.fillWidth: true
                                    text: appRow.app.name || appRow.modelData
                                    color: "#1f2d3a"
                                    font.bold: true
                                    wrapMode: Text.Wrap
                                }
                                Label {
                                    visible: !appRow.app.installed
                                    text: qsTr("Not installed")
                                    color: "#5d6d7e"
                                    font: Kirigami.Theme.smallFont
                                }
                            }
                            Repeater {
                                model: kiosk.optionKeys
                                Item {
                                    id: cell
                                    required property string modelData
                                    required property int index
                                    Layout.preferredWidth: kiosk.cellWidth
                                    implicitHeight: check.implicitHeight
                                    CardCheck {
                                        id: check
                                        anchors.centerIn: parent
                                        enabled: appRow.usable
                                        checked: kiosk.optionOn(appRow.app, cell.modelData)
                                        Accessible.name: kiosk.optionTexts[cell.index] + " · " + (appRow.app.name || "")
                                        onToggled: {
                                            kiosk.setOption(appRow.app, cell.modelData, checked)
                                            // A click breaks the binding: the state stays the page's
                                            checked = Qt.binding(() => kiosk.optionOn(appRow.app, cell.modelData))
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // --------------------------------------------------------------------- apply
    RowLayout {
        visible: kiosk.found
        Layout.fillWidth: true
        spacing: 10

        Label {
            Layout.fillWidth: true
            text: kiosk.working ? qsTr("Changing the kiosk…")
                  : (kiosk.changed ? qsTr("Not applied yet.") : "")
            color: "#5d6d7e"
            wrapMode: Text.Wrap
        }
        CardButton {
            visible: kiosk.changed && !kiosk.working
            text: qsTr("Undo")
            onClicked: kiosk.undo()
        }
        // The page's primary button only while there is something to apply
        CardButton {
            highlighted: kiosk.ownsPrimary
            text: qsTr("Apply")
            enabled: kiosk.changed && kiosk.canApply && !kiosk.working
                     && !(kiosk.profilesChanged && kiosk.busy !== "")
            onClicked: kiosk.apply()
        }
    }

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: kiosk.message !== ""
        type: kiosk.failed ? Kirigami.MessageType.Error : Kirigami.MessageType.Positive
        text: kiosk.message
    }
}

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.plasmoid

// AstroArch Pi Monitor: shows what the astroarch-pi-monitor daemon reports.
// The widget only displays: the thresholds and the notifications are the daemon's
// (settings page of this widget, or `astroarch-pi-monitor set`). Backend.qml reads the
// daemon through Plasma's D-Bus module, or through the command without the daemon.
PlasmoidItem {
    id: root

    Backend {
        id: backend
        pollSeconds: Plasmoid.configuration.pollSeconds
    }

    // As `astroarch-pi-monitor status --json` prints it
    readonly property var status: backend.status
    readonly property bool failed: backend.failed

    readonly property int level: status.level !== undefined ? status.level : 0
    readonly property bool hasTemp: status.temperature !== undefined && status.temperature !== null
    readonly property bool hasVolt: status.voltage !== undefined && status.voltage !== null
    readonly property bool hasFan: status.fan_rpm !== undefined && status.fan_rpm !== null
    readonly property bool voltIsSupply: status.voltage_kind === "ext5v"
    readonly property var alerts: status.alerts !== undefined ? status.alerts : []

    readonly property color levelColor: level >= 2 ? Kirigami.Theme.negativeTextColor
                                      : level === 1 ? Kirigami.Theme.neutralTextColor
                                      : Kirigami.Theme.textColor

    function fmtTemp(v) {
        return (v === undefined || v === null) ? "--" : v.toFixed(1) + " °C";
    }
    // "2450 rpm (50 %)": the speed, and what the system asks of the fan
    function fmtFan() {
        if (!hasFan)
            return "--";
        const pct = (status.fan_percent !== undefined && status.fan_percent !== null)
                    ? " (" + status.fan_percent + " %)" : "";
        return status.fan_rpm + " rpm" + pct;
    }
    function fmtVolt(v) {
        return (v === undefined || v === null) ? "--" : v.toFixed(2) + " V";
    }

    // A thermometer: orange for a warning, red when critical (icons of this package)
    function alertIcon(lvl) {
        return lvl >= 2 ? "org.astroarch.pimonitor-critical" : "org.astroarch.pimonitor-warning";
    }

    Plasmoid.icon: level > 0 ? alertIcon(level) : "temperature-normal"
    Plasmoid.status: level > 0 ? PlasmaCore.Types.NeedsAttentionStatus : PlasmaCore.Types.ActiveStatus

    toolTipMainText: status.model ? status.model : i18n("Raspberry Pi")
    toolTipSubText: alerts.length > 0 ? alerts.join("\n")
                  : failed ? i18n("The monitor does not answer")
                  : fmtTemp(status.temperature) + " · " + fmtVolt(status.voltage)

    compactRepresentation: MouseArea {
        id: compact
        hoverEnabled: true
        onClicked: root.expanded = !root.expanded

        Layout.minimumWidth: row.implicitWidth + Kirigami.Units.smallSpacing * 2
        Layout.preferredWidth: Layout.minimumWidth

        RowLayout {
            id: row
            anchors.centerIn: parent
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                source: Plasmoid.icon
                implicitWidth: Kirigami.Units.iconSizes.smallMedium
                implicitHeight: Kirigami.Units.iconSizes.smallMedium
                visible: root.level > 0
            }
            PlasmaComponents3.Label {
                color: root.levelColor
                text: {
                    let t = root.hasTemp ? Math.round(root.status.temperature) + "°C" : "--";
                    if (Plasmoid.configuration.showVoltage && root.voltIsSupply && root.hasVolt)
                        t += " · " + root.status.voltage.toFixed(2) + "V";
                    if (Plasmoid.configuration.showFan && root.hasFan)
                        t += " · " + root.status.fan_rpm + "rpm";
                    return t;
                }
            }
        }
    }

    fullRepresentation: ColumnLayout {
        Layout.minimumWidth: Kirigami.Units.gridUnit * 16
        // As tall as what it shows, no empty space under it
        Layout.minimumHeight: implicitHeight
        Layout.maximumHeight: implicitHeight
        spacing: Kirigami.Units.largeSpacing

        Kirigami.Heading {
            level: 3
            text: root.status.model ? root.status.model : i18n("Raspberry Pi")
            Layout.fillWidth: true
            elide: Text.ElideRight
        }

        GridLayout {
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing
            Layout.fillWidth: true

            PlasmaComponents3.Label { text: i18n("Temperature") }
            PlasmaComponents3.Label {
                text: root.fmtTemp(root.status.temperature)
                font.bold: true
                color: root.hasTemp && root.status.temperature >= root.status.thresholds.temp_crit ? Kirigami.Theme.negativeTextColor
                     : root.hasTemp && root.status.temperature >= root.status.thresholds.temp_warn ? Kirigami.Theme.neutralTextColor
                     : Kirigami.Theme.textColor
            }

            PlasmaComponents3.Label {
                text: root.voltIsSupply ? i18n("5 V input") : i18n("Core voltage")
                visible: root.hasVolt
            }
            PlasmaComponents3.Label {
                text: root.fmtVolt(root.status.voltage)
                font.bold: true
                visible: root.hasVolt
                color: root.voltIsSupply && root.status.voltage <= root.status.thresholds.volt_crit ? Kirigami.Theme.negativeTextColor
                     : root.voltIsSupply && root.status.voltage <= root.status.thresholds.volt_warn ? Kirigami.Theme.neutralTextColor
                     : Kirigami.Theme.textColor
            }

            PlasmaComponents3.Label { text: i18n("Fan"); visible: root.hasFan }
            PlasmaComponents3.Label {
                text: root.fmtFan()
                font.bold: true
                visible: root.hasFan
            }
        }

        // The active alerts, in the language of the session (from the daemon)
        Repeater {
            model: root.alerts
            delegate: RowLayout {
                required property string modelData
                Layout.fillWidth: true
                Kirigami.Icon {
                    source: root.alertIcon(root.level)
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: Kirigami.Units.iconSizes.small
                }
                PlasmaComponents3.Label {
                    text: modelData
                    color: root.levelColor
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }
            }
        }

        PlasmaComponents3.Label {
            visible: root.alerts.length === 0 && !root.failed
            text: i18n("All values are normal")
            color: Kirigami.Theme.positiveTextColor
        }
        PlasmaComponents3.Label {
            visible: root.status.undervoltage_past === true && root.status.undervoltage_now !== true
            text: i18n("An under-voltage happened since the boot")
            color: Kirigami.Theme.neutralTextColor
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }
        PlasmaComponents3.Label {
            visible: root.status.daemon === false
            text: i18n("The monitor service is not running: no notifications. Start it with: systemctl --user start astroarch-pi-monitor")
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
            opacity: 0.8
        }
        PlasmaComponents3.Label {
            visible: root.failed
            text: i18n("The monitor does not answer")
            color: Kirigami.Theme.negativeTextColor
        }
    }
}

import QtQml
import org.astroarch.pimonitor

// The QML plugin backend (crates/astroarch-pi-monitor-qml): PiMonitor polls the service in
// a thread of the Plasma shell, over one D-Bus connection, and exposes typed properties.
// Here they are put in the shape of `status --json`, which is what the widget reads.
QtObject {
    id: backend

    property int pollSeconds: 2
    readonly property bool failed: false
    readonly property bool usesPlugin: true

    signal applied(bool ok, string message)

    function apply(tempWarn, tempCrit, voltWarn, voltCrit, notifications) {
        monitor.applyThresholds(tempWarn, tempCrit, voltWarn, voltCrit, notifications);
    }

    readonly property var status: monitor.ready ? ({
        "model": monitor.model,
        "temperature": isNaN(monitor.temperature) ? null : monitor.temperature,
        "voltage": isNaN(monitor.voltage) ? null : monitor.voltage,
        "voltage_kind": monitor.voltageKind,
        "fan_rpm": monitor.fanRpm < 0 ? null : monitor.fanRpm,
        "fan_percent": monitor.fanPercent < 0 ? null : monitor.fanPercent,
        "undervoltage_now": monitor.undervoltageNow,
        "undervoltage_past": monitor.undervoltagePast,
        "throttled": monitor.throttled,
        "soft_temp_limit": monitor.softTempLimit,
        "level": monitor.level,
        "alerts": Array.from(monitor.alerts),
        "thresholds": {
            "temp_warn": monitor.tempWarn,
            "temp_crit": monitor.tempCrit,
            "volt_warn": monitor.voltWarn,
            "volt_crit": monitor.voltCrit
        },
        "notifications": monitor.notifications,
        "daemon": monitor.daemonRunning
    }) : ({})

    onPollSecondsChanged: monitor.setInterval(pollSeconds)

    property PiMonitor monitor: PiMonitor {
        Component.onCompleted: start(backend.pollSeconds)
        onApplied: (ok, message) => backend.applied(ok, message)
    }
}

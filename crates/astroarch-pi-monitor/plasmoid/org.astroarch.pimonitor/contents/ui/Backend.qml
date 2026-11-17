import QtQuick

// What the widget uses to read the monitor and to change its settings. It tries the QML
// plugin (BackendPlugin.qml: typed properties, one D-Bus connection, no process) and
// falls back to the command (BackendExec.qml) when the plugin is not installed, or not in
// the QML import path of the running Plasma shell (a first install under a user account:
// log in again). Both have the same interface, so the widget works at once, and switches
// to the plugin by itself when it is there.
QtObject {
    id: backend

    // Seconds between two readings
    property int pollSeconds: 2

    // The status, as `astroarch-pi-monitor status --json` prints it (see status.rs)
    readonly property var status: impl ? impl.status : ({})
    // The monitor does not answer (command backend only)
    readonly property bool failed: impl ? impl.failed : false
    // Which one is in use
    readonly property bool usesPlugin: impl ? impl.usesPlugin : false

    // The answer to apply(): `message` is empty on success, else the reason
    signal applied(bool ok, string message)

    function apply(tempWarn, tempCrit, voltWarn, voltCrit, notifications) {
        if (impl)
            impl.apply(tempWarn, tempCrit, voltWarn, voltCrit, notifications);
    }

    property QtObject impl: null

    property Binding pollBinding: Binding {
        target: backend.impl
        property: "pollSeconds"
        value: backend.pollSeconds
        when: backend.impl !== null
    }
    property Connections appliedRelay: Connections {
        target: backend.impl
        function onApplied(ok, message) {
            backend.applied(ok, message);
        }
    }

    function load(file) {
        const component = Qt.createComponent(file, Component.PreferSynchronous);
        if (component.status !== Component.Ready)
            return null;
        return component.createObject(backend, { "pollSeconds": backend.pollSeconds });
    }

    Component.onCompleted: {
        // A missing plugin makes BackendPlugin.qml fail to load: Qt logs a warning, no more
        impl = load("BackendPlugin.qml");
        if (impl === null)
            impl = load("BackendExec.qml");
    }
}

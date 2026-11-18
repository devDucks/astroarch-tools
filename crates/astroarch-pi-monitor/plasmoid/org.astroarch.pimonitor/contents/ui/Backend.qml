import QtQuick
import org.kde.plasma.plasma5support as P5Support
import org.kde.plasma.workspace.dbus as DBus

// How the widget reads the monitor and changes its settings. It asks the service on the
// session bus with Plasma's own D-Bus module (no plugin of ours inside plasmashell, no
// process started). When the service is not running, it runs the command instead:
// `astroarch-pi-monitor status --json` reads the sensors directly, `set` writes the file.
QtObject {
    id: backend

    // Replaced by the absolute path of the binary when the widget is installed
    readonly property string bin: "@BIN@"

    // Seconds between two readings
    property int pollSeconds: 2

    // The status, as `astroarch-pi-monitor status --json` prints it (see status.rs)
    property var status: ({})
    // Neither the service nor the command answers
    property bool failed: false

    // The answer to apply(): `message` is empty on success, else the reason
    signal applied(bool ok, string message)

    function message(member, args) {
        const msg = {
            "service": "org.astroarch.PiMonitor",
            "path": "/org/astroarch/PiMonitor",
            "iface": "org.astroarch.PiMonitor1",
            "member": member
        };
        // No "signature": with one, Plasma 6.8 sends the call without its arguments. The
        // typed values (DBus.double, DBus.bool) give the signature
        if (args)
            msg["arguments"] = args;
        return msg;
    }

    // The service is not on the bus (not running, not installed)
    function notRunning(reply) {
        const name = reply && reply.error ? reply.error.name : "";
        return name === "org.freedesktop.DBus.Error.ServiceUnknown"
            || name === "org.freedesktop.DBus.Error.NameHasNoOwner";
    }

    function poll() {
        DBus.SessionBus.asyncCall(message("GetStatus"), reply => {
            // The reply's value is the JSON string (a D-Bus string, wrapped)
            const v = reply.value;
            try {
                backend.status = JSON.parse(typeof v === "string" ? v : v.value);
                backend.failed = false;
            } catch (e) {
                backend.failed = true;
            }
        }, reply => {
            // No service: a direct reading through the command
            runner.run("status --json");
        });
    }

    function apply(tempWarn, tempCrit, voltWarn, voltCrit, notifications) {
        const args = [new DBus.double(tempWarn), new DBus.double(tempCrit),
                      new DBus.double(voltWarn), new DBus.double(voltCrit),
                      new DBus.bool(notifications)];
        DBus.SessionBus.asyncCall(message("SetConfig", args), reply => {
            backend.applied(true, "");
            backend.poll();
        }, reply => {
            if (!backend.notRunning(reply)) {
                backend.applied(false, reply.error.message);
                return;
            }
            // No service: the command writes the file, read at the next start
            runner.run("set --temp-warn " + tempWarn.toFixed(1) + " --temp-crit " + tempCrit.toFixed(1)
                       + " --volt-warn " + voltWarn.toFixed(2) + " --volt-crit " + voltCrit.toFixed(2)
                       + " --notifications " + (notifications ? "on" : "off"));
        });
    }

    property Timer timer: Timer {
        interval: Math.max(1, backend.pollSeconds) * 1000
        repeat: true
        running: true
        triggeredOnStart: true
        onTriggered: backend.poll()
    }

    // The command, for when the service is not running
    property P5Support.DataSource runner: P5Support.DataSource {
        engine: "executable"
        interval: 0

        function shellQuote(s) {
            return "'" + s.replace(/'/g, "'\\''") + "'";
        }
        function run(args) {
            const cmd = shellQuote(backend.bin) + " " + args;
            // Still running from the last time: do not start it twice
            if (connectedSources.indexOf(cmd) < 0)
                connectSource(cmd);
        }

        onNewData: (sourceName, data) => {
            disconnectSource(sourceName);
            const ok = data["exit code"] === 0;
            if (sourceName.endsWith(" status --json")) {
                try {
                    backend.status = ok ? JSON.parse(data["stdout"]) : ({});
                    backend.failed = !ok;
                } catch (e) {
                    backend.failed = true;
                }
            } else {
                backend.applied(ok, ok ? "" : data["stderr"].trim());
            }
        }
    }
}

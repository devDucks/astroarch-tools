import QtQuick
import org.kde.plasma.plasma5support as P5Support

// The command backend: runs `astroarch-pi-monitor status --json` every few seconds (the
// executable engine of Plasma5Support), and `astroarch-pi-monitor set ...` to change the
// settings. Same interface as BackendPlugin.qml.
QtObject {
    id: backend

    // Replaced by the absolute path of the binary when the widget is installed
    readonly property string bin: "@BIN@"

    property int pollSeconds: 2
    property var status: ({})
    property bool failed: false
    readonly property bool usesPlugin: false

    signal applied(bool ok, string message)

    function shellQuote(s) {
        return "'" + s.replace(/'/g, "'\\''") + "'";
    }

    function apply(tempWarn, tempCrit, voltWarn, voltCrit, notifications) {
        const cmd = shellQuote(bin) + " set"
            + " --temp-warn " + tempWarn.toFixed(1)
            + " --temp-crit " + tempCrit.toFixed(1)
            + " --volt-warn " + voltWarn.toFixed(2)
            + " --volt-crit " + voltCrit.toFixed(2)
            + " --notifications " + (notifications ? "on" : "off");
        runner.connectSource(cmd);
    }

    property P5Support.DataSource poller: P5Support.DataSource {
        engine: "executable"
        connectedSources: [backend.shellQuote(backend.bin) + " status --json"]
        interval: backend.pollSeconds * 1000

        onNewData: (sourceName, data) => {
            if (data["exit code"] !== 0) {
                backend.failed = true;
                return;
            }
            try {
                backend.status = JSON.parse(data["stdout"]);
                backend.failed = false;
            } catch (e) {
                backend.failed = true;
            }
        }
    }

    property P5Support.DataSource runner: P5Support.DataSource {
        engine: "executable"
        interval: 0

        onNewData: (sourceName, data) => {
            disconnectSource(sourceName);
            if (data["exit code"] === 0)
                backend.applied(true, "");
            else
                backend.applied(false, data["stderr"].trim());
        }
    }
}

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kcmutils as KCM
import org.kde.kirigami as Kirigami

// The widget's settings. The widget's own ones (cfg_*) are saved by Plasma; the thresholds
// and the notifications belong to the daemon (they apply to every widget and to the
// notifications even when no widget is shown): Plasma's Apply and OK send them to it
// through Backend.qml (saveConfig() below, which Plasma calls before saving).
KCM.SimpleKCM {
    id: page

    property alias cfg_pollSeconds: pollSpin.value
    property alias cfg_showVoltage: showVoltageBox.checked
    property alias cfg_showFan: showFanBox.checked
    property int cfg_pollSecondsDefault: 2
    property bool cfg_showVoltageDefault: true
    property bool cfg_showFanDefault: false

    // The numbers are integers in the spin boxes, scaled by 10 (°C) or 100 (V)
    property bool loaded: false

    Backend {
        id: backend

        onApplied: (ok, message) => {
            result.type = ok ? Kirigami.MessageType.Positive : Kirigami.MessageType.Error;
            result.text = ok ? i18n("Thresholds applied.") : message;
            result.visible = true;
        }
    }

    // The current status; the settings are filled in once, when the first one arrives
    readonly property var current: backend.status
    // "pi5", "pi4", "pi" or "generic" (see status.rs); empty until the first status
    readonly property string board: current.board !== undefined ? current.board : ""
    // Only a Raspberry Pi 5 measures its 5 V input: the voltage thresholds apply there only
    readonly property bool hasSupply: board === "pi5"

    onCurrentChanged: {
        if (loaded || current.thresholds === undefined)
            return;
        tempWarn.value = Math.round(current.thresholds.temp_warn * 10);
        tempCrit.value = Math.round(current.thresholds.temp_crit * 10);
        voltWarn.value = Math.round(current.thresholds.volt_warn * 100);
        voltCrit.value = Math.round(current.thresholds.volt_crit * 100);
        notifyBox.checked = current.notifications;
        loaded = true;
    }

    // Emitted when the user changes a threshold or the notifications: Plasma enables Apply
    // (and asks before closing), until the next Apply. Not an `unsavedChanges` property:
    // Plasma only looks at its change signal, so one that stays true after an Apply would
    // never enable Apply again
    signal configurationChanged()

    function edited() {
        if (loaded)
            configurationChanged();
    }

    // The thresholds or the notifications differ from the daemon's
    function differs() {
        return loaded && current.thresholds !== undefined
            && (tempWarn.value !== Math.round(current.thresholds.temp_warn * 10)
                || tempCrit.value !== Math.round(current.thresholds.temp_crit * 10)
                || voltWarn.value !== Math.round(current.thresholds.volt_warn * 100)
                || voltCrit.value !== Math.round(current.thresholds.volt_crit * 100)
                || notifyBox.checked !== current.notifications);
    }

    // Called by Plasma on Apply and on OK, before it saves the cfg_ settings
    function saveConfig() {
        if (!differs())
            return;
        result.visible = false;
        backend.apply(tempWarn.value / 10, tempCrit.value / 10,
                      voltWarn.value / 100, voltCrit.value / 100, notifyBox.checked);
    }

    component DecimalSpinBox: QQC2.SpinBox {
        id: sb
        property int decimals: 1
        property string suffix: ""
        readonly property int factor: Math.pow(10, decimals)
        editable: true
        stepSize: 5
        // A change by the user (not the values filled in from the daemon)
        onValueModified: page.edited()
        validator: DoubleValidator {
            bottom: Math.min(sb.from, sb.to) / sb.factor
            top: Math.max(sb.from, sb.to) / sb.factor
            decimals: sb.decimals
            notation: DoubleValidator.StandardNotation
        }
        textFromValue: function (v, locale) {
            return Number(v / factor).toLocaleString(locale, 'f', decimals) + suffix;
        }
        valueFromText: function (text, locale) {
            return Math.round(Number.fromLocaleString(locale, text.replace(suffix, "").trim()) * factor);
        }
    }

    Kirigami.FormLayout {
        // A Pi 5 whose 5 V input cannot be read: vcgencmd is missing or has no access
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.hasSupply && page.current.voltage_kind !== "ext5v"
            type: Kirigami.MessageType.Warning
            text: i18n("The 5 V input cannot be read: vcgencmd is missing, or cannot open /dev/vcio. Both come with the package raspberrypi-utils.")
        }
        // A Pi 4 (or older) has no sensor on its input
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.board === "pi4" || page.board === "pi"
            type: Kirigami.MessageType.Information
            text: i18n("This board cannot measure its 5 V input: there are no voltage thresholds. The under-voltage reported by the firmware still raises an alert.")
        }

        Item { Kirigami.FormData.isSection: true; Kirigami.FormData.label: i18n("Temperature") }

        DecimalSpinBox {
            id: tempWarn
            Kirigami.FormData.label: i18n("Warning from:")
            from: 300; to: 1100; decimals: 1; suffix: " °C"
        }
        DecimalSpinBox {
            id: tempCrit
            Kirigami.FormData.label: i18n("Critical from:")
            from: 300; to: 1100; decimals: 1; suffix: " °C"
        }

        Item {
            visible: page.hasSupply
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: i18n("5 V input voltage")
        }

        DecimalSpinBox {
            id: voltWarn
            visible: page.hasSupply
            Kirigami.FormData.label: i18n("Warning under:")
            from: 300; to: 550; decimals: 2; suffix: " V"
        }
        DecimalSpinBox {
            id: voltCrit
            visible: page.hasSupply
            Kirigami.FormData.label: i18n("Critical under:")
            from: 300; to: 550; decimals: 2; suffix: " V"
        }

        Item { Kirigami.FormData.isSection: true }

        QQC2.CheckBox {
            id: notifyBox
            Kirigami.FormData.label: i18n("Notifications:")
            text: i18n("Notify when a threshold is exceeded")
            onToggled: page.edited()
        }

        Kirigami.InlineMessage {
            id: result
            Layout.fillWidth: true
            visible: false
        }

        Item { Kirigami.FormData.isSection: true; Kirigami.FormData.label: i18n("Widget") }

        QQC2.SpinBox {
            id: pollSpin
            Kirigami.FormData.label: i18n("Refresh every:")
            from: 1; to: 60
            textFromValue: function (v, locale) { return i18np("%1 second", "%1 seconds", v); }
            valueFromText: function (text, locale) { return parseInt(text); }
        }
        QQC2.CheckBox {
            id: showVoltageBox
            visible: page.hasSupply
            text: i18n("Show the 5 V voltage next to the temperature in the panel")
        }
        QQC2.CheckBox {
            id: showFanBox
            text: i18n("Show the fan speed in the panel")
        }
    }
}

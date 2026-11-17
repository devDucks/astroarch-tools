import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kcmutils as KCM
import org.kde.kirigami as Kirigami

// The widget's settings. The widget's own ones (cfg_*) are saved by Plasma; the thresholds
// and the notifications belong to the daemon (they apply to every widget and to the
// notifications even when no widget is shown): "Apply the thresholds" sends them to it
// (through Backend.qml).
KCM.SimpleKCM {
    id: page

    property alias cfg_pollSeconds: pollSpin.value
    property alias cfg_showVoltage: showVoltageBox.checked
    property alias cfg_showFan: showFanBox.checked

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

    // The current settings, filled in once when the first status arrives
    readonly property var current: backend.status
    onCurrentChanged: {
        if (loaded || current.thresholds === undefined)
            return;
        tempWarn.value = Math.round(current.thresholds.temp_warn * 10);
        tempCrit.value = Math.round(current.thresholds.temp_crit * 10);
        voltWarn.value = Math.round(current.thresholds.volt_warn * 100);
        voltCrit.value = Math.round(current.thresholds.volt_crit * 100);
        notifyBox.checked = current.notifications;
        supplyNote.visible = current.voltage_kind !== "ext5v";
        loaded = true;
    }

    function apply() {
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
        Kirigami.InlineMessage {
            id: supplyNote
            Layout.fillWidth: true
            visible: false
            type: Kirigami.MessageType.Information
            text: i18n("This board cannot measure its 5 V input (Raspberry Pi 4): the voltage thresholds are not used. The under-voltage reported by the firmware still raises an alert.")
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

        Item { Kirigami.FormData.isSection: true; Kirigami.FormData.label: i18n("5 V input voltage (Raspberry Pi 5)") }

        DecimalSpinBox {
            id: voltWarn
            Kirigami.FormData.label: i18n("Warning under:")
            from: 300; to: 550; decimals: 2; suffix: " V"
        }
        DecimalSpinBox {
            id: voltCrit
            Kirigami.FormData.label: i18n("Critical under:")
            from: 300; to: 550; decimals: 2; suffix: " V"
        }

        Item { Kirigami.FormData.isSection: true }

        QQC2.CheckBox {
            id: notifyBox
            Kirigami.FormData.label: i18n("Notifications:")
            text: i18n("Notify when a threshold is exceeded")
        }

        QQC2.Button {
            text: i18n("Apply the thresholds")
            icon.name: "dialog-ok-apply"
            enabled: page.loaded
            onClicked: page.apply()
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
            text: i18n("Show the 5 V voltage next to the temperature in the panel")
        }
        QQC2.CheckBox {
            id: showFanBox
            text: i18n("Show the fan speed in the panel")
        }
    }
}

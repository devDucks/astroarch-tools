# astroarch-pi-monitor

Temperature, voltage and fan of the Raspberry Pi (4 and 5), alerts when a threshold is
exceeded, and a Plasma 6 widget to see it all. Rust (zbus, tokio) for the service and
for the QML plugin (cxx-qt), QML (Qt 6, Kirigami) for the widget, no hand-written C++.

```
 sensors ──► astroarch-pi-monitor daemon ──► notifications (org.freedesktop.Notifications)
 (sysfs,        │  org.astroarch.PiMonitor
  vcgencmd)     │  on the session bus
                ▼
   astroarch-pi-monitor status --json  ◄─┐
   astroarch-pi-monitor set ...        ◄─┤ Plasma widget, through the QML plugin
   QML plugin org.astroarch.pimonitor  ◄─┘ (crate astroarch-pi-monitor-qml) or, without it, the command
```

## Install

    cargo build --release -p astroarch-pi-monitor -p astroarch-pi-monitor-qml
    target/release/astroarch-pi-monitor install

(the plugin needs the Qt 6 development files and `lld`: `sudo pacman -S lld`; without the
plugin, `cargo build --release -p astroarch-pi-monitor` is enough and the widget uses the
command). That is all: it copies the binary to `~/.local/bin`, puts the widget in
`~/.local/share/plasma/plasmoids/org.astroarch.pimonitor`, the QML plugin in
`~/.local/lib/qt6/qml/org/astroarch/pimonitor`, writes the systemd user service and starts
it. Then right-click the desktop or a panel → *Add Widgets…* →
*AstroArch Pi Monitor* (restart the Plasma shell if it is not listed yet:
`systemctl --user restart plasma-plasmashell`).

`install --system` (as root) puts the binary in `/usr/local/bin`, the widget in
`/usr/share/plasma/plasmoids`, the plugin in `/usr/lib/qt6/qml/org/astroarch/pimonitor` and
the service in `/etc/systemd/user`, enabled for every user from their next login. This is
the install to prefer for the plugin: it is found as soon as the Plasma shell restarts. `uninstall [--system]` removes it all (the settings are kept).
The widget and the unit are embedded in the binary; for a package, the same files are in
`plasmoid/` and `systemd/` (replace `@BIN@` by the binary's path in the three files that
have it).

## What is monitored

| | Raspberry Pi 5 | Raspberry Pi 4 |
|---|---|---|
| SoC temperature | `/sys/class/thermal/thermal_zone0/temp` | same |
| Voltage | the 5 V input, `vcgencmd pmic_read_adc EXT5V_V` | no input sensor: the core voltage is shown, for information |
| Fan | `pwmfan` hwmon device (the Active Cooler on the fan connector): speed in rpm, and the duty cycle the system asks for | no sensor: nothing is shown |
| Under-voltage now / since boot, throttling | `vcgencmd get_throttled`, and the kernel's `rpi_volt` hwmon device | same |

Alerts, each with a hysteresis so a value hovering around a threshold does not flap
(2 °C, 0.05 V):

| Alert | Level | Default |
|---|---|---|
| Temperature | warning / critical | from 70 °C / from 80 °C |
| 5 V input voltage (Pi 5) | warning / critical | under 4.75 V / under 4.63 V |
| Under-voltage reported by the firmware | critical | |
| CPU throttled by the firmware | warning | |
| Fan not turning: asked to turn (duty cycle above 0) and 0 rpm for 5 samples in a row (10 s) | warning | |

The Raspberry Pi firmware throttles from 80 °C and flags an under-voltage under about
4.63 V; the warnings come before. On a Pi 4 only the firmware's under-voltage flag raises a
voltage alert.

## Notifications

Sent by the daemon (so also without the widget): when an alert appears or gets worse, and
every 10 minutes while something is critical (`repeat_critical_minutes`, 0 to send each
only once). One notification is replaced, not stacked, and closed when everything is back
to normal. Critical ones stay until closed. If the notification server is not up yet (early
in the session), it is retried 30 s later. Texts are in English, French, German, Italian
and Spanish, after the session's language.

## Settings

`~/.config/astroarch/pi-monitor.toml` (created by `install`):

    [thresholds]
    temp_warn = 70.0
    temp_crit = 80.0
    volt_warn = 4.75
    volt_crit = 4.63

    [notifications]
    enabled = true
    repeat_critical_minutes = 10

    [monitor]
    interval_secs = 2

Change them in the widget's settings page (*Configure…* → *Apply the thresholds*), or:

    astroarch-pi-monitor set --temp-warn 65 --temp-crit 78 --notifications on

The running daemon applies them at once and saves them; without a daemon the file is
written. A file edited by hand is read at the next start.

## Commands

    astroarch-pi-monitor daemon             the D-Bus service (what the systemd unit runs)
    astroarch-pi-monitor status [--json]    what the service reports; a direct reading if it is not running
    astroarch-pi-monitor set [options]      thresholds and notifications
    astroarch-pi-monitor install|uninstall [--system]

## D-Bus

Session bus, `org.astroarch.PiMonitor`, `/org/astroarch/PiMonitor`,
interface `org.astroarch.PiMonitor1`:

- properties `Temperature` (d, NaN if unknown), `Voltage` (d, NaN if unknown),
  `FanRpm` and `FanPercent` (i, -1 if there is no fan sensor),
  `Level` (u: 0 ok, 1 warning, 2 critical), `Message` (s, one alert per line); they emit
  `PropertiesChanged`
- `GetStatus() → s`: the JSON of `status --json`
- `SetConfig(d tempWarn, d tempCrit, d voltWarn, d voltCrit, b notifications)`
- signal `AlertChanged(u level, s message)`

Another AstroArch tool can read them, e.g.
`busctl --user get-property org.astroarch.PiMonitor /org/astroarch/PiMonitor org.astroarch.PiMonitor1 Level`.

## The widget

`plasmoid/org.astroarch.pimonitor`: a Plasma 6 package (QML). It has no logic of its own: it
shows the temperature and the voltage in the panel (red or orange from the thresholds), with
the details and the active alerts when opened. Its settings page edits the thresholds. A
plasmoid is a QML package that plasmashell loads, so it cannot be Rust itself; the Rust
service and the Rust plugin are what it displays.

`Backend.qml` is how it reaches the monitor, and it chooses by itself:

- **the QML plugin** (`BackendPlugin.qml`): the `PiMonitor` object, with typed properties,
  one D-Bus connection kept open, no process started;
- **the command** (`BackendExec.qml`), when the plugin is not installed or not in the Plasma
  shell's QML import path: every 2 s it runs `astroarch-pi-monitor status --json`
  (Plasma5Support's `executable` engine).

So the widget works right after `install`, and uses the plugin from the moment the shell
finds it. A *user* install puts the plugin in `~/.local/lib/qt6/qml` and a script in
`~/.config/plasma-workspace/env/` that adds it to `QML_IMPORT_PATH`: the shell reads that at
the start of the session, so the plugin is used from the next login.

## The QML plugin

`crates/astroarch-pi-monitor-qml`, a cxx-qt dynamic plugin (`org.astroarch.pimonitor`),
usable by any QML, e.g. another AstroArch tool:

    import org.astroarch.pimonitor
    PiMonitor {
        Component.onCompleted: start(2)      // seconds between two readings
        // properties: temperature, voltage (NaN if unknown), voltageKind, fanRpm and
        // fanPercent (-1 if there is no fan sensor), level, alerts,
        // undervoltageNow, undervoltagePast, throttled, softTempLimit, model, daemonRunning,
        // ready, tempWarn, tempCrit, voltWarn, voltCrit, notifications
        // invokables: setInterval(secs), applyThresholds(tw, tc, vw, vc, notifications)
        // signal: applied(ok, message)
    }

A thread of the host (plasmashell) asks the service for its status and hands it to the Qt
thread; without the service, the sensors are read directly (`daemonRunning` is false). The
plugin is loaded *into plasmashell*: a crash in it takes the panel down, which is why it
does nothing but this.

Building a dynamic plugin from Cargo alone needs two things the crate handles in `build.rs`:
the library is named after the module (`liborg_astroarch_pimonitor.so`), and the linker must
be lld (a second version script exports `qt_plugin_instance` and
`qt_plugin_query_metadata_v2`, which a Rust `cdylib` would otherwise hide; GNU ld and gold
refuse a second script). Without `ld.lld`, the build warns and the plugin does not load.
It is built for one Qt: rebuild it after a Qt upgrade.

## Tests

    cargo test -p astroarch-pi-monitor

The alert levels, the hysteresis, the fan alert, the notification policy, the parsing of the sensors, the
settings and the installer's files are unit tested. The plugin and the widget's QML are not
covered by `cargo test`: the plugin was loaded in Qt's `qml` runner against a running daemon,
and the widget needs a Plasma session.

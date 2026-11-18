# astroarch-pi-monitor

Temperature, voltage and fan of the Raspberry Pi (4 and 5), alerts when a threshold is
exceeded, and a Plasma 6 widget to see it all. Rust (zbus, tokio) for the service, QML
(Qt 6, Kirigami) for the widget.

```
 sensors ──► astroarch-pi-monitor daemon ──► notifications (org.freedesktop.Notifications)
 (sysfs,        │  org.astroarch.PiMonitor
  vcgencmd)     │  on the session bus
                ▼
   Plasma widget (Plasma's D-Bus module)
   astroarch-pi-monitor status --json / set ...  (the widget's fallback without the service)
```

## Install

    cargo build --release -p astroarch-pi-monitor
    target/release/astroarch-pi-monitor install

That is all: it copies the binary to `~/.local/bin`, puts the widget in
`~/.local/share/plasma/plasmoids/org.astroarch.pimonitor`, writes the systemd user service
and starts it. Then right-click the desktop or a panel → *Add Widgets…* →
*AstroArch Pi Monitor* (restart the Plasma shell if it is not listed yet:
`systemctl --user restart plasma-plasmashell`).

`install --system` (as root) puts the binary in `/usr/local/bin`, the widget in
`/usr/share/plasma/plasmoids` and the service in `/etc/systemd/user`, enabled for every
user from their next login. `uninstall [--system]` removes it all (the settings are kept).
The widget and the unit are embedded in the binary; for a package, the same files are in
`plasmoid/` and `systemd/` (replace `@BIN@` by the binary's path in the two files that
have it: `contents/ui/Backend.qml` and the unit).

## What is monitored

| | Raspberry Pi 5 | Raspberry Pi 4 | PC (x86) |
|---|---|---|---|
| CPU temperature | `cpu_thermal` hwmon device (= `thermal_zone0`) | same | `coretemp` (Intel, *Package id 0*), `k10temp` / `zenpower` (AMD, *Tdie*, else *Tctl*), else the `x86_pkg_temp` thermal zone |
| Voltage | the 5 V input, `vcgencmd pmic_read_adc EXT5V_V` | no input sensor: the core voltage is shown, for information | none |
| Fan | `pwmfan` hwmon device (the Active Cooler on the fan connector): speed in rpm, and the duty cycle the system asks for | no sensor: nothing is shown | the first hwmon fan that turns, speed only |
| Under-voltage now / since boot, throttling | `vcgencmd get_throttled`, and the kernel's `rpi_volt` hwmon device | same | none |

The board is told by the device tree (`brcm,bcm2712`: Pi 5, 500, CM5; `brcm,bcm2711`: Pi 4,
400, CM4). `vcgencmd` is only run on a Raspberry Pi; it comes with the package
`raspberrypi-utils`, whose udev rule also lets the `video` group (the desktop user) use
`/dev/vcio`. Without it, a Pi shows the temperature and the fan only.

Alerts, each with a hysteresis so a value hovering around a threshold does not flap
(2 °C, 0.05 V):

| Alert | Level | Default |
|---|---|---|
| Temperature | warning / critical | from 70 °C / from 80 °C (PC: 90 °C / 98 °C) |
| 5 V input voltage (Pi 5) | warning / critical | under 4.75 V / under 4.63 V |
| Under-voltage reported by the firmware | critical | |
| CPU throttled by the firmware | warning | |
| Fan not turning: asked to turn (duty cycle above 0) and 0 rpm for 5 samples in a row (10 s) | warning | |

The Raspberry Pi firmware throttles from 80 °C and flags an under-voltage under about
4.63 V; the warnings come before. An x86 CPU runs much hotter under load (87 °C for a
Ryzen 7 8845HS mini PC during a compile) and throttles itself at 100 °C, hence its own
defaults. On a Pi 4 only the firmware's under-voltage flag raises a
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
    temp_warn = 70.0        # 90.0 on a PC
    temp_crit = 80.0        # 98.0 on a PC
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
- signal `AlertChanged(u level, s message)`, when an alert appears, goes or changes level

Another AstroArch tool can read them, e.g.
`busctl --user get-property org.astroarch.PiMonitor /org/astroarch/PiMonitor org.astroarch.PiMonitor1 Level`.

## The widget

`plasmoid/org.astroarch.pimonitor`: a Plasma 6 package (QML). It has no logic of its own: it
shows the temperature and the voltage in the panel (red or orange from the thresholds), with
the details and the active alerts when opened. Its settings page edits the thresholds. A
plasmoid is a QML package that plasmashell loads, so it cannot be Rust itself; the Rust
service is what it displays.

`Backend.qml` is how it reaches the monitor: it calls `GetStatus` (and `SetConfig`) on the
session bus with Plasma's own D-Bus module, `org.kde.plasma.workspace.dbus`
(`SessionBus.asyncCall`). Nothing of ours is loaded into the Plasma shell, no process is
started, and nothing needs a rebuild after a Qt update. When the service is not running,
it runs `astroarch-pi-monitor status --json` instead (a direct reading of the sensors), and
`set` writes the settings to the file.

One catch of that module (Plasma 6.8): a call given a `signature` is sent without its
arguments. `Backend.qml` passes typed values (`DBus.double`, `DBus.bool`) and no signature.

## Tests

    cargo test -p astroarch-pi-monitor

The alert levels, the hysteresis, the fan alert, the notification policy, the parsing of the sensors, the
settings and the installer's files are unit tested. The widget's QML is not covered by
`cargo test`: it needs a Plasma session.

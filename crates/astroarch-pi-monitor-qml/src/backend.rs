//! CXX-Qt bridge: `PiMonitor` for QML.
//!
//!     import org.astroarch.pimonitor
//!     PiMonitor { Component.onCompleted: start(2) }   // then: temperature, voltage, level, alerts...
//!
//! A thread (with its own small tokio runtime) asks the monitor service for its status over
//! one D-Bus connection every few seconds, and hands it to the Qt thread. If the service is
//! not running, the sensors are read directly (`daemonRunning` is then false). Nothing here
//! blocks the Qt thread, and nothing panics in a callback called by Qt: this code runs
//! inside plasmashell.

use astroarch_pi_monitor::client::{self, Changes};
use astroarch_pi_monitor::status::Status;
use core::pin::Pin;
use cxx_qt::{CxxQtThread, Threading};
use cxx_qt_lib::{QString, QStringList};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        // The service answers (false: the sensors are read directly, no notifications)
        #[qproperty(bool, daemon_running, cxx_name = "daemonRunning")]
        // A first status was received
        #[qproperty(bool, ready)]
        #[qproperty(QString, model)]
        // °C and V; NaN when the sensor cannot be read
        #[qproperty(f64, temperature)]
        #[qproperty(f64, voltage)]
        // "ext5v" (the 5 V input, Pi 5), "core" (the core voltage, Pi 4) or "none"
        #[qproperty(QString, voltage_kind, cxx_name = "voltageKind")]
        // Fan speed in rpm and what the system asks of it in percent; -1 without a fan sensor
        #[qproperty(i32, fan_rpm, cxx_name = "fanRpm")]
        #[qproperty(i32, fan_percent, cxx_name = "fanPercent")]
        // 0 ok, 1 warning, 2 critical
        #[qproperty(i32, level)]
        // The active alerts, in the language of the session
        #[qproperty(QStringList, alerts)]
        #[qproperty(bool, undervoltage_now, cxx_name = "undervoltageNow")]
        #[qproperty(bool, undervoltage_past, cxx_name = "undervoltagePast")]
        #[qproperty(bool, throttled)]
        #[qproperty(bool, soft_temp_limit, cxx_name = "softTempLimit")]
        // The settings in force
        #[qproperty(f64, temp_warn, cxx_name = "tempWarn")]
        #[qproperty(f64, temp_crit, cxx_name = "tempCrit")]
        #[qproperty(f64, volt_warn, cxx_name = "voltWarn")]
        #[qproperty(f64, volt_crit, cxx_name = "voltCrit")]
        #[qproperty(bool, notifications)]
        type PiMonitor = super::PiMonitorRust;

        /// Starts the polling (once); `interval_secs` between two readings
        #[qinvokable]
        fn start(self: Pin<&mut PiMonitor>, interval_secs: i32);

        /// Changes the interval between two readings
        #[qinvokable]
        #[cxx_name = "setInterval"]
        fn set_interval(self: &PiMonitor, interval_secs: i32);

        /// Changes the thresholds and the notifications; `applied` tells the result
        #[qinvokable]
        #[cxx_name = "applyThresholds"]
        fn apply_thresholds(
            self: &PiMonitor,
            temp_warn: f64,
            temp_crit: f64,
            volt_warn: f64,
            volt_crit: f64,
            notifications: bool,
        );

        /// The answer to `applyThresholds`: `message` is empty on success, else the reason
        #[qsignal]
        fn applied(self: Pin<&mut PiMonitor>, ok: bool, message: &QString);
    }

    impl cxx_qt::Threading for PiMonitor {}
}

enum Command {
    Interval(u64),
    Apply(Changes),
}

pub struct PiMonitorRust {
    daemon_running: bool,
    ready: bool,
    model: QString,
    temperature: f64,
    voltage: f64,
    voltage_kind: QString,
    fan_rpm: i32,
    fan_percent: i32,
    level: i32,
    alerts: QStringList,
    undervoltage_now: bool,
    undervoltage_past: bool,
    throttled: bool,
    soft_temp_limit: bool,
    temp_warn: f64,
    temp_crit: f64,
    volt_warn: f64,
    volt_crit: f64,
    notifications: bool,
    /// To the polling thread; dropped with the object, which ends the thread
    commands: Option<UnboundedSender<Command>>,
}

impl Default for PiMonitorRust {
    fn default() -> Self {
        Self {
            daemon_running: false,
            ready: false,
            model: QString::default(),
            temperature: f64::NAN,
            voltage: f64::NAN,
            voltage_kind: QString::from("none"),
            fan_rpm: -1,
            fan_percent: -1,
            level: 0,
            alerts: QStringList::default(),
            undervoltage_now: false,
            undervoltage_past: false,
            throttled: false,
            soft_temp_limit: false,
            temp_warn: 70.0,
            temp_crit: 80.0,
            volt_warn: 4.75,
            volt_crit: 4.63,
            notifications: true,
            commands: None,
        }
    }
}

impl qobject::PiMonitor {
    pub fn start(mut self: Pin<&mut Self>, interval_secs: i32) {
        use cxx_qt::CxxQtType;
        if self.rust().commands.is_some() {
            return;
        }
        let (tx, rx) = unbounded_channel();
        self.as_mut().rust_mut().commands = Some(tx);
        let qt_thread = self.qt_thread();
        let secs = interval_secs.clamp(1, 60) as u64;
        let spawned = std::thread::Builder::new().name("pi-monitor".into()).spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                rt.block_on(run(qt_thread, rx, secs));
            }
        });
        if spawned.is_err() {
            eprintln!("astroarch-pi-monitor: cannot start the polling thread");
        }
    }

    pub fn set_interval(&self, interval_secs: i32) {
        use cxx_qt::CxxQtType;
        if let Some(tx) = &self.rust().commands {
            let _ = tx.send(Command::Interval(interval_secs.clamp(1, 60) as u64));
        }
    }

    pub fn apply_thresholds(
        &self,
        temp_warn: f64,
        temp_crit: f64,
        volt_warn: f64,
        volt_crit: f64,
        notifications: bool,
    ) {
        use cxx_qt::CxxQtType;
        let changes = Changes {
            temp_warn: Some(temp_warn),
            temp_crit: Some(temp_crit),
            volt_warn: Some(volt_warn),
            volt_crit: Some(volt_crit),
            notifications: Some(notifications),
        };
        if let Some(tx) = &self.rust().commands {
            let _ = tx.send(Command::Apply(changes));
        }
    }
}

/// The polling thread
async fn run(qt: CxxQtThread<qobject::PiMonitor>, mut rx: UnboundedReceiver<Command>, mut secs: u64) {
    let mut conn: Option<zbus::Connection> = None;
    loop {
        // One connection for all the readings; opened again if it could not be
        if conn.is_none() {
            conn = zbus::Connection::session().await.ok();
        }
        let status = match &conn {
            Some(c) => match client::daemon_status_on(c).await {
                Some(s) => s,
                None => client::direct_status().await,
            },
            None => client::direct_status().await,
        };
        // The object is gone (the widget was removed): stop
        if qt.queue(move |obj| show(obj, &status)).is_err() {
            return;
        }

        let sleep = tokio::time::sleep(Duration::from_secs(secs));
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                command = rx.recv() => match command {
                    None => return,
                    Some(Command::Interval(s)) => { secs = s; break; }
                    Some(Command::Apply(changes)) => {
                        let result = client::set(changes).await;
                        let (ok, message) = match result {
                            Ok(_) => (true, String::new()),
                            Err(e) => (false, e),
                        };
                        if qt.queue(move |mut obj| obj.as_mut().applied(ok, &QString::from(&message))).is_err() {
                            return;
                        }
                        // Read again at once: the new thresholds apply to the alerts
                        break;
                    }
                }
            }
        }
    }
}

/// Hands a status to the properties (on the Qt thread). A property only emits its
/// change signal when its value changed
fn show(mut obj: Pin<&mut qobject::PiMonitor>, st: &Status) {
    obj.as_mut().set_daemon_running(st.daemon);
    obj.as_mut().set_ready(true);
    obj.as_mut().set_model(QString::from(&st.model));
    obj.as_mut().set_temperature(st.temperature.unwrap_or(f64::NAN));
    obj.as_mut().set_voltage(st.voltage.unwrap_or(f64::NAN));
    obj.as_mut().set_voltage_kind(QString::from(&st.voltage_kind));
    obj.as_mut().set_fan_rpm(st.fan_rpm.map_or(-1, |v| v as i32));
    obj.as_mut().set_fan_percent(st.fan_percent.map_or(-1, |v| v as i32));
    obj.as_mut().set_level(st.level as i32);
    obj.as_mut().set_alerts(st.alerts.iter().map(|a| QString::from(a)).collect());
    obj.as_mut().set_undervoltage_now(st.undervoltage_now);
    obj.as_mut().set_undervoltage_past(st.undervoltage_past);
    obj.as_mut().set_throttled(st.throttled);
    obj.as_mut().set_soft_temp_limit(st.soft_temp_limit);
    obj.as_mut().set_temp_warn(st.thresholds.temp_warn);
    obj.as_mut().set_temp_crit(st.thresholds.temp_crit);
    obj.as_mut().set_volt_warn(st.thresholds.volt_warn);
    obj.as_mut().set_volt_crit(st.thresholds.volt_crit);
    obj.as_mut().set_notifications(st.notifications);
}

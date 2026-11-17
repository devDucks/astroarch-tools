//! The daemon: samples the sensors, evaluates the alerts, sends the notifications, and
//! serves the result on the session bus.
//!
//!   bus name     org.astroarch.PiMonitor
//!   object       /org/astroarch/PiMonitor
//!   interface    org.astroarch.PiMonitor1
//!
//!   properties   Temperature (d, NaN if unknown), Voltage (d, NaN if unknown),
//!                Level (u: 0 ok, 1 warning, 2 critical), Message (s, one alert per line),
//!                FanRpm (i, -1 if no fan sensor), FanPercent (i, -1 if no fan sensor)
//!   methods      GetStatus() -> s (JSON, see `Status`)
//!                SetConfig(d tempWarn, d tempCrit, d voltWarn, d voltCrit, b notifications)
//!   signal       AlertChanged(u level, s message)

use crate::alerts::{Evaluator, Level, NotifyPolicy, Thresholds};
use crate::config::Config;
use crate::i18n::{self, Lang};
use crate::sensors::{board_model, Sensors};
use crate::status::Status;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;
use zbus::{connection, fdo, interface, proxy};

pub const BUS_NAME: &str = "org.astroarch.PiMonitor";
pub const OBJECT_PATH: &str = "/org/astroarch/PiMonitor";
pub const INTERFACE: &str = "org.astroarch.PiMonitor1";

struct State {
    config: Config,
    status: Status,
}

struct PiMonitor {
    state: Arc<RwLock<State>>,
    /// Wakes the sampling loop up (new settings: evaluate again now)
    wake: Arc<Notify>,
}

#[interface(name = "org.astroarch.PiMonitor1")]
impl PiMonitor {
    #[zbus(property)]
    fn temperature(&self) -> f64 {
        self.state.read().unwrap().status.temperature.unwrap_or(f64::NAN)
    }

    #[zbus(property)]
    fn voltage(&self) -> f64 {
        self.state.read().unwrap().status.voltage.unwrap_or(f64::NAN)
    }

    #[zbus(property)]
    fn level(&self) -> u32 {
        self.state.read().unwrap().status.level as u32
    }

    #[zbus(property)]
    fn fan_rpm(&self) -> i32 {
        self.state.read().unwrap().status.fan_rpm.map_or(-1, |v| v as i32)
    }

    #[zbus(property)]
    fn fan_percent(&self) -> i32 {
        self.state.read().unwrap().status.fan_percent.map_or(-1, |v| v as i32)
    }

    #[zbus(property)]
    fn message(&self) -> String {
        self.state.read().unwrap().status.alerts.join("\n")
    }

    fn get_status(&self) -> String {
        serde_json::to_string(&self.state.read().unwrap().status).unwrap_or_default()
    }

    fn set_config(
        &self,
        temp_warn: f64,
        temp_crit: f64,
        volt_warn: f64,
        volt_crit: f64,
        notifications: bool,
    ) -> fdo::Result<()> {
        let mut cfg = self.state.read().unwrap().config.clone();
        cfg.thresholds = Thresholds { temp_warn, temp_crit, volt_warn, volt_crit };
        cfg.notifications.enabled = notifications;
        cfg.validate().map_err(fdo::Error::InvalidArgs)?;
        cfg.save().map_err(fdo::Error::Failed)?;
        self.state.write().unwrap().config = cfg;
        self.wake.notify_one();
        Ok(())
    }

    #[zbus(signal)]
    async fn alert_changed(emitter: &SignalEmitter<'_>, level: u32, message: &str) -> zbus::Result<()>;
}

/// org.freedesktop.Notifications, the part we use
#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, &Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;
}

async fn notify(
    proxy: &NotificationsProxy<'_>,
    replaces: u32,
    level: Level,
    title: &str,
    body: &str,
) -> zbus::Result<u32> {
    let critical = level == Level::Critical;
    let urgency = Value::U8(if critical { 2 } else { 1 });
    let entry = Value::from("org.astroarch.pimonitor");
    let hints = HashMap::from([("urgency", &urgency), ("desktop-entry", &entry)]);
    proxy
        .notify(
            "AstroArch Pi Monitor",
            replaces,
            if critical { "dialog-error" } else { "dialog-warning" },
            title,
            body,
            &[],
            hints,
            // A critical alert stays until it is closed; a warning goes away by itself
            if critical { 0 } else { 15_000 },
        )
        .await
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load();
    config.validate()?;
    let model = board_model(Path::new("/"));
    let state = Arc::new(RwLock::new(State { config, status: Status::default() }));
    let wake = Arc::new(Notify::new());

    let conn = connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, PiMonitor { state: state.clone(), wake: wake.clone() })?
        .build()
        .await
        .map_err(|e| format!("cannot own {BUS_NAME} on the session bus (already running?): {e}"))?;
    let notifications = NotificationsProxy::new(&conn).await?;
    let iface_ref = conn.object_server().interface::<_, PiMonitor>(OBJECT_PATH).await?;

    let mut sensors = Sensors::default();
    let mut evaluator = Evaluator::default();
    let mut policy = NotifyPolicy::default();
    let mut notification_id = 0u32;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut previous: Option<Status> = None;

    eprintln!("astroarch-pi-monitor: started ({})", if model.is_empty() { "unknown board" } else { &model });

    loop {
        let (cfg, lang) = (state.read().unwrap().config.clone(), Lang::detect());
        let reading = sensors.read().await;
        let eval = evaluator.evaluate(&reading, &cfg.thresholds);
        let status = Status::build(&model, &reading, &eval, &cfg, lang, true);
        state.write().unwrap().status = status.clone();

        // D-Bus: properties and signal, only for what changed
        let changed = previous.as_ref().is_none_or(|p| {
            p.temperature != status.temperature
                || p.voltage != status.voltage
                || p.fan_rpm != status.fan_rpm
                || p.fan_percent != status.fan_percent
                || p.level != status.level
                || p.alerts != status.alerts
        });
        if changed {
            let emitter = iface_ref.signal_emitter();
            let iface = iface_ref.get().await;
            if let Err(e) = async {
                iface.temperature_changed(emitter).await?;
                iface.voltage_changed(emitter).await?;
                iface.fan_rpm_changed(emitter).await?;
                iface.fan_percent_changed(emitter).await?;
                iface.level_changed(emitter).await?;
                iface.message_changed(emitter).await
            }
            .await
            {
                eprintln!("astroarch-pi-monitor: cannot emit the property changes: {e}");
            }
        }
        if previous.as_ref().is_none_or(|p| p.level != status.level || p.alerts != status.alerts) {
            if previous.is_some() || status.level > 0 {
                let what = if status.alerts.is_empty() { "back to normal".to_string() } else { status.alerts.join(" | ") };
                eprintln!("astroarch-pi-monitor: level {}: {what}", status.level);
            }
            let _ = PiMonitor::alert_changed(iface_ref.signal_emitter(), status.level as u32, &status.alerts.join("\n")).await;
        }

        // Notifications
        let now = Instant::now();
        let repeat = (cfg.notifications.repeat_critical_minutes > 0)
            .then(|| Duration::from_secs(cfg.notifications.repeat_critical_minutes * 60));
        if eval.alerts.is_empty() {
            // Back to normal: the stale notification goes away
            if notification_id != 0 {
                let _ = notifications.close_notification(notification_id).await;
                notification_id = 0;
            }
            policy.due(&eval.alerts, now, repeat);
        } else if cfg.notifications.enabled && policy.due(&eval.alerts, now, repeat) {
            let body = status.alerts.join("\n");
            match notify(&notifications, notification_id, eval.level, i18n::title(lang, eval.level), &body).await {
                Ok(id) => {
                    notification_id = id;
                    policy.sent(&eval.alerts, now);
                }
                Err(e) => {
                    eprintln!("astroarch-pi-monitor: notification failed: {e}");
                    policy.failed(now);
                }
            }
        }

        previous = Some(status);

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(cfg.monitor.interval_secs)) => {}
            _ = wake.notified() => {}
            _ = tokio::signal::ctrl_c() => break,
            _ = sigterm.recv() => break,
        }
    }
    eprintln!("astroarch-pi-monitor: stopped");
    Ok(())
}

//! `astroarch-pi-monitor status` and `set`: they talk to the daemon on the session bus,
//! and still work when it is not running (the values are then read directly and the
//! settings written to the file). The Plasma widget calls them.

use crate::alerts::Evaluator;
use crate::config::Config;
use crate::daemon::{BUS_NAME, INTERFACE, OBJECT_PATH};
use crate::i18n::Lang;
use crate::sensors::{board_model, Board, Sensors};
use crate::status::Status;
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;
use zbus::proxy;

#[proxy(interface = "org.astroarch.PiMonitor1")]
trait PiMonitorDaemon {
    fn get_status(&self) -> zbus::Result<String>;
    fn set_config(
        &self,
        temp_warn: f64,
        temp_crit: f64,
        volt_warn: f64,
        volt_crit: f64,
        notifications: bool,
    ) -> zbus::Result<()>;
}

/// A proxy to the daemon on an existing session bus connection
async fn proxy_on(conn: &zbus::Connection) -> Option<PiMonitorDaemonProxy<'static>> {
    PiMonitorDaemonProxy::builder(conn)
        .destination(BUS_NAME)
        .ok()?
        .path(OBJECT_PATH)
        .ok()?
        .interface(INTERFACE)
        .ok()?
        .build()
        .await
        .ok()
}

/// A proxy to the daemon on the session bus (the bus exists even when the daemon does not)
async fn connect() -> Option<PiMonitorDaemonProxy<'static>> {
    proxy_on(&zbus::Connection::session().await.ok()?).await
}

/// The daemon's answer on this connection, if it runs
pub async fn daemon_status_on(conn: &zbus::Connection) -> Option<Status> {
    let json = timeout(Duration::from_millis(1500), proxy_on(conn).await?.get_status()).await.ok()?.ok()?;
    serde_json::from_str(&json).ok()
}

/// The daemon's answer, if it runs
async fn daemon_status() -> Option<Status> {
    daemon_status_on(&zbus::Connection::session().await.ok()?).await
}

/// Reads the sensors here, without the daemon
pub async fn direct_status() -> Status {
    let cfg = Config::load();
    let reading = Sensors::default().read().await;
    let eval = Evaluator::default().evaluate(&reading, &cfg.thresholds);
    Status::build(Board::this(), &board_model(Path::new("/")), &reading, &eval, &cfg, Lang::detect(), false)
}

pub async fn status() -> Status {
    match daemon_status().await {
        Some(s) => s,
        None => direct_status().await,
    }
}

/// What `set` changes; `None` keeps the current value
#[derive(Debug, Default)]
pub struct Changes {
    pub temp_warn: Option<f64>,
    pub temp_crit: Option<f64>,
    pub volt_warn: Option<f64>,
    pub volt_crit: Option<f64>,
    pub notifications: Option<bool>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.temp_warn.is_none()
            && self.temp_crit.is_none()
            && self.volt_warn.is_none()
            && self.volt_crit.is_none()
            && self.notifications.is_none()
    }
}

/// Applies the changes (validated first); returns the settings now in force
pub async fn set(changes: Changes) -> Result<Config, String> {
    // Start from what is in force: the daemon's settings, else the file's
    let mut cfg = Config::load();
    let daemon = daemon_status().await;
    if let Some(s) = &daemon {
        cfg.thresholds = s.thresholds;
        cfg.notifications.enabled = s.notifications;
    }
    let t = &mut cfg.thresholds;
    t.temp_warn = changes.temp_warn.unwrap_or(t.temp_warn);
    t.temp_crit = changes.temp_crit.unwrap_or(t.temp_crit);
    t.volt_warn = changes.volt_warn.unwrap_or(t.volt_warn);
    t.volt_crit = changes.volt_crit.unwrap_or(t.volt_crit);
    cfg.notifications.enabled = changes.notifications.unwrap_or(cfg.notifications.enabled);
    cfg.validate()?;

    if daemon.is_none() {
        // No daemon: write the file, it is read at the next start
        cfg.save()?;
        return Ok(cfg);
    }
    // The daemon applies the settings at once and saves them (it validates them too)
    let t = cfg.thresholds;
    let proxy = connect().await.ok_or("cannot reach the monitor service on the session bus")?;
    timeout(
        Duration::from_millis(3000),
        proxy.set_config(t.temp_warn, t.temp_crit, t.volt_warn, t.volt_crit, cfg.notifications.enabled),
    )
    .await
    .map_err(|_| "the monitor service does not answer".to_string())?
    .map_err(|e| e.to_string())?;
    Ok(cfg)
}

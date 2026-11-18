//! The status: what the daemon answers to `GetStatus`, what `astroarch-pi-monitor status
//! --json` prints, and what the Plasma widget reads.

use crate::alerts::{Evaluation, Thresholds};
use crate::config::Config;
use crate::i18n::{self, Lang};
use crate::sensors::{Board, Reading};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    /// "Raspberry Pi 5 Model B Rev 1.0", or empty
    pub model: String,
    /// "pi5", "pi4", "pi" (an older Raspberry Pi) or "generic" (x86...)
    pub board: String,
    /// °C, null if it could not be read
    pub temperature: Option<f64>,
    /// V, null if it could not be read
    pub voltage: Option<f64>,
    /// "ext5v" (the 5 V input, Pi 5), "core" (the SoC core voltage, Pi 4) or "none"
    pub voltage_kind: String,
    pub undervoltage_now: bool,
    /// An under-voltage happened since the boot
    pub undervoltage_past: bool,
    pub throttled: bool,
    pub soft_temp_limit: bool,
    /// Fan speed in rpm, null if there is no fan sensor (Pi 4)
    pub fan_rpm: Option<u32>,
    /// What the system asks of the fan, in percent of its full speed
    pub fan_percent: Option<u32>,
    /// 0 ok, 1 warning, 2 critical
    pub level: u8,
    /// The active alerts, in the language of the session
    pub alerts: Vec<String>,
    pub thresholds: Thresholds,
    pub notifications: bool,
    /// false when the daemon is not running and the values were read directly
    pub daemon: bool,
}

impl Status {
    pub fn build(
        board: Board,
        model: &str,
        r: &Reading,
        eval: &Evaluation,
        cfg: &Config,
        lang: Lang,
        daemon: bool,
    ) -> Status {
        Status {
            model: model.to_string(),
            board: board.as_str().to_string(),
            temperature: r.temperature,
            voltage: r.voltage,
            voltage_kind: r.voltage_kind.as_str().to_string(),
            undervoltage_now: r.undervoltage_now,
            undervoltage_past: r.undervoltage_past,
            throttled: r.throttled_now,
            soft_temp_limit: r.soft_temp_limit,
            fan_rpm: r.fan_rpm,
            fan_percent: r.fan_percent,
            level: eval.level as u8,
            alerts: eval.alerts.iter().map(|a| i18n::alert_text(lang, a)).collect(),
            thresholds: cfg.thresholds,
            notifications: cfg.notifications.enabled,
            daemon,
        }
    }

    /// A few lines for a terminal
    pub fn to_text(&self) -> String {
        let t = self.temperature.map_or("--".to_string(), |v| format!("{v:.1} °C"));
        let v = self.voltage.map_or("--".to_string(), |v| format!("{v:.2} V"));
        let what = match self.voltage_kind.as_str() {
            "ext5v" => "5 V input",
            "core" => "core voltage",
            _ => "voltage",
        };
        let state = match self.level {
            0 => "ok",
            1 => "warning",
            _ => "CRITICAL",
        };
        let mut out = String::new();
        if !self.model.is_empty() {
            out.push_str(&format!("{}\n", self.model));
        }
        out.push_str(&format!("temperature  {t}\n"));
        // A PC has no supply voltage to show
        if self.board != "generic" {
            out.push_str(&format!("{what:<12} {v}\n"));
        }
        if let Some(rpm) = self.fan_rpm {
            let pct = self.fan_percent.map_or(String::new(), |p| format!(" ({p} %)"));
            out.push_str(&format!("fan          {rpm} rpm{pct}\n"));
        }
        out.push_str(&format!("state        {state}\n"));
        for a in &self.alerts {
            out.push_str(&format!("  ! {a}\n"));
        }
        if self.undervoltage_past && !self.undervoltage_now {
            out.push_str("  (an under-voltage happened since the boot)\n");
        }
        if !self.daemon {
            out.push_str("(the daemon is not running: no notifications)\n");
        }
        out
    }
}

//! The settings, in `~/.config/astroarch/pi-monitor.toml`. The daemon is the only writer
//! while it runs (the widget and `astroarch-pi-monitor set` go through D-Bus).

use crate::alerts::Thresholds;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const HEADER: &str = "\
# AstroArch Pi Monitor settings.
# Edited by the widget's settings page and by `astroarch-pi-monitor set`;
# a change made by hand is read at the next start of the daemon.
#
# thresholds: temperature in °C; voltage in V, on the 5 V input (Raspberry Pi 5 only).
#   The Raspberry Pi firmware throttles from 80 °C and flags an under-voltage under ~4.63 V;
#   on a PC (x86) the defaults are 90 and 98 °C.
# notifications: repeat_critical_minutes = 0 sends each notification only once.
";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub enabled: bool,
    /// While something is critical, notify again every this many minutes (0: never)
    pub repeat_critical_minutes: u64,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { enabled: true, repeat_critical_minutes: 10 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Monitor {
    /// Seconds between two samples of the sensors
    pub interval_secs: u64,
}

impl Default for Monitor {
    fn default() -> Self {
        Self { interval_secs: 2 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub thresholds: Thresholds,
    pub notifications: Notifications,
    pub monitor: Monitor,
}

/// The defaults of this machine (its thresholds depend on it, see `Thresholds::for_board`)
impl Default for Config {
    fn default() -> Self {
        Config {
            thresholds: Thresholds::for_this_board(),
            notifications: Notifications::default(),
            monitor: Monitor::default(),
        }
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/root/.config"));
    base.join("astroarch/pi-monitor.toml")
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        self.thresholds.validate()?;
        if !(1..=60).contains(&self.monitor.interval_secs) {
            return Err("the sampling interval must be between 1 and 60 seconds".into());
        }
        Ok(())
    }

    pub fn from_toml(text: &str) -> Result<Config, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// The saved settings; the defaults if there is no file, or if it is not valid (said on
    /// stderr: a typo in the file must not stop the monitoring)
    pub fn load() -> Config {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => Config::from_toml(&text).unwrap_or_else(|e| {
                eprintln!("{}: {e}; using the default settings", path.display());
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn to_toml(&self) -> Result<String, String> {
        let body = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        Ok(format!("{HEADER}\n{body}"))
    }

    /// Writes the file through a temporary one, so a crash cannot leave half a file
    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        let text = self.to_toml()?;
        let dir = path.parent().ok_or("no directory for the settings")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(Config::default().validate().is_ok());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut c = Config::default();
        c.thresholds.temp_warn = 65.0;
        c.notifications.enabled = false;
        let text = c.to_toml().unwrap();
        assert!(text.starts_with("# AstroArch Pi Monitor settings."));
        assert_eq!(Config::from_toml(&text).unwrap(), c);
    }

    #[test]
    fn a_partial_file_gets_the_defaults_for_the_rest() {
        let c = Config::from_toml("[thresholds]\ntemp_warn = 65.0\n").unwrap();
        assert_eq!(c.thresholds.temp_warn, 65.0);
        assert_eq!(c.thresholds.temp_crit, Thresholds::for_this_board().temp_crit);
        assert_eq!(c.notifications, Notifications::default());
    }

    #[test]
    fn invalid_files_are_rejected() {
        // Over the critical default of a Pi (80) and of a PC (98)
        assert!(Config::from_toml("[thresholds]\ntemp_warn = 99.0\n").is_err());
        assert!(Config::from_toml("[monitor]\ninterval_secs = 0\n").is_err());
        assert!(Config::from_toml("not toml at all [").is_err());
    }
}

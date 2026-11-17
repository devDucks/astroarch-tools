//! Raspberry Pi sensors: SoC temperature, supply voltage, firmware throttling flags.
//!
//! - temperature: `/sys/class/thermal/thermal_zone0/temp` (millidegrees Celsius)
//! - Pi 5: the 5 V input rail, `vcgencmd pmic_read_adc EXT5V_V`
//! - Pi 4: no input voltage sensor; the core voltage (`vcgencmd measure_volts core`) is
//!   reported instead, and under-voltage is known from the firmware flag only
//! - fan (Pi 5, official Active Cooler): the `pwmfan` hwmon device, `fan1_input` (rpm) and
//!   `pwm1` (duty cycle, 0-255); a Pi 4 has no fan sensor
//! - under-voltage now / since boot, throttling: `vcgencmd get_throttled`, and the kernel's
//!   `rpi_volt` hwmon device (`in0_lcrit_alarm`) for "under-voltage now" without vcgencmd

use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// What the voltage in a [`Reading`] measures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoltageKind {
    /// No voltage could be read
    #[default]
    None,
    /// The 5 V power input (Raspberry Pi 5): thresholds apply
    Supply5V,
    /// The SoC core voltage (Raspberry Pi 4): informative only
    Core,
}

impl VoltageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VoltageKind::None => "none",
            VoltageKind::Supply5V => "ext5v",
            VoltageKind::Core => "core",
        }
    }
}

/// One sample of every sensor; what could not be read stays `None` / `false`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    pub temperature: Option<f64>,
    pub voltage: Option<f64>,
    pub voltage_kind: VoltageKind,
    pub undervoltage_now: bool,
    pub undervoltage_past: bool,
    pub throttled_now: bool,
    pub soft_temp_limit: bool,
    /// Fan speed in rpm, and what the system asks of it in percent of the full speed
    pub fan_rpm: Option<u32>,
    pub fan_percent: Option<u32>,
}

// Bits of `vcgencmd get_throttled`
const UNDERVOLTAGE_NOW: u32 = 1 << 0;
const THROTTLED_NOW: u32 = 1 << 2;
const SOFT_TEMP_LIMIT_NOW: u32 = 1 << 3;
const UNDERVOLTAGE_PAST: u32 = 1 << 16;

/// "52300" (millidegrees) -> 52.3
pub fn parse_millidegrees(s: &str) -> Option<f64> {
    let v: f64 = s.trim().parse().ok()?;
    Some(v / 1000.0)
}

/// "EXT5V_V volt(24)=4.97650000V" or "volt=0.8600V" -> volts
pub fn parse_volts(s: &str) -> Option<f64> {
    let v: f64 = s.split('=').nth(1)?.trim().trim_end_matches('V').trim().parse().ok()?;
    v.is_finite().then_some(v)
}

/// "throttled=0x50005" -> bits
pub fn parse_throttled(s: &str) -> Option<u32> {
    let hex = s.trim().strip_prefix("throttled=0x")?;
    u32::from_str_radix(hex.trim(), 16).ok()
}

/// The `rpi_volt` hwmon device: `in0_lcrit_alarm` is 1 while the supply is under-voltage
fn hwmon_undervoltage(hwmon_root: &Path) -> Option<bool> {
    for entry in std::fs::read_dir(hwmon_root).ok()?.flatten() {
        let dir = entry.path();
        let is_rpi_volt = std::fs::read_to_string(dir.join("name"))
            .map(|n| n.trim() == "rpi_volt")
            .unwrap_or(false);
        if is_rpi_volt {
            if let Ok(v) = std::fs::read_to_string(dir.join("in0_lcrit_alarm")) {
                return Some(v.trim() == "1");
            }
        }
    }
    None
}

/// "128" (a duty cycle out of 255) -> 50 (percent)
pub fn pwm_to_percent(pwm: u32) -> u32 {
    ((pwm.min(255) * 100 + 127) / 255).min(100)
}

/// The `pwmfan` hwmon device: (rpm, percent), whichever could be read
fn hwmon_fan(hwmon_root: &Path) -> Option<(Option<u32>, Option<u32>)> {
    for entry in std::fs::read_dir(hwmon_root).ok()?.flatten() {
        let dir = entry.path();
        let is_fan = std::fs::read_to_string(dir.join("name"))
            .map(|n| n.trim() == "pwmfan")
            .unwrap_or(false);
        if !is_fan {
            continue;
        }
        let read = |file: &str| -> Option<u32> {
            std::fs::read_to_string(dir.join(file)).ok()?.trim().parse().ok()
        };
        let (rpm, percent) = (read("fan1_input"), read("pwm1").map(pwm_to_percent));
        if rpm.is_some() || percent.is_some() {
            return Some((rpm, percent));
        }
    }
    None
}

/// Where the sensors are read; `root` is `/` except in tests
pub struct Sensors {
    root: PathBuf,
    /// Set after the first failed `pmic_read_adc`: this board has no PMIC ADC (Pi 4), do
    /// not start a process for it at every sample
    pmic_unavailable: bool,
}

impl Default for Sensors {
    fn default() -> Self {
        Self::with_root("/")
    }
}

impl Sensors {
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), pmic_unavailable: false }
    }

    fn read_temperature(&self) -> Option<f64> {
        let p = self.root.join("sys/class/thermal/thermal_zone0/temp");
        parse_millidegrees(&std::fs::read_to_string(p).ok()?)
    }

    pub async fn read(&mut self) -> Reading {
        let mut r = Reading { temperature: self.read_temperature(), ..Reading::default() };

        if !self.pmic_unavailable {
            match vcgencmd(&["pmic_read_adc", "EXT5V_V"]).await.and_then(|s| parse_volts(&s)) {
                Some(v) => {
                    r.voltage = Some(v);
                    r.voltage_kind = VoltageKind::Supply5V;
                }
                None => self.pmic_unavailable = true,
            }
        }
        if r.voltage.is_none() {
            if let Some(v) = vcgencmd(&["measure_volts", "core"]).await.and_then(|s| parse_volts(&s)) {
                r.voltage = Some(v);
                r.voltage_kind = VoltageKind::Core;
            }
        }

        if let Some(bits) = vcgencmd(&["get_throttled"]).await.and_then(|s| parse_throttled(&s)) {
            r.undervoltage_now = bits & UNDERVOLTAGE_NOW != 0;
            r.undervoltage_past = bits & UNDERVOLTAGE_PAST != 0;
            r.throttled_now = bits & THROTTLED_NOW != 0;
            r.soft_temp_limit = bits & SOFT_TEMP_LIMIT_NOW != 0;
        }
        if let Some((rpm, percent)) = hwmon_fan(&self.root.join("sys/class/hwmon")) {
            r.fan_rpm = rpm;
            r.fan_percent = percent;
        }
        // The kernel flag is the same information without a process; it also covers a system
        // where vcgencmd is not installed
        if let Some(true) = hwmon_undervoltage(&self.root.join("sys/class/hwmon")) {
            r.undervoltage_now = true;
        }
        r
    }
}

/// Runs `vcgencmd` (in the PATH, or in /opt/vc/bin) and returns its stdout on success.
/// A hung firmware call must not block the daemon, hence the timeout
async fn vcgencmd(args: &[&str]) -> Option<String> {
    for program in ["vcgencmd", "/opt/vc/bin/vcgencmd"] {
        let child = Command::new(program).args(args).kill_on_drop(true).output();
        match timeout(Duration::from_secs(2), child).await {
            Ok(Ok(out)) if out.status.success() => {
                return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
            }
            // Started but failed (unknown command, no /dev/vcio access...): no point in
            // trying the other location
            Ok(Ok(_)) | Err(_) => return None,
            // Not found here: try the next location
            Ok(Err(_)) => continue,
        }
    }
    None
}

/// The board's model, e.g. "Raspberry Pi 5 Model B Rev 1.0" (empty if unknown)
pub fn board_model(root: &Path) -> String {
    std::fs::read_to_string(root.join("proc/device-tree/model"))
        .map(|s| s.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_temperature() {
        assert_eq!(parse_millidegrees("52300\n"), Some(52.3));
        assert_eq!(parse_millidegrees("abc"), None);
    }

    #[test]
    fn parses_volts() {
        assert_eq!(parse_volts("EXT5V_V volt(24)=4.97650000V"), Some(4.9765));
        assert_eq!(parse_volts("volt=0.8600V"), Some(0.86));
        assert_eq!(parse_volts("error=1 error_msg=\"Command not registered\""), None);
        assert_eq!(parse_volts("volt=nanV"), None);
    }

    #[test]
    fn parses_throttled() {
        assert_eq!(parse_throttled("throttled=0x0"), Some(0));
        let bits = parse_throttled("throttled=0x50005").unwrap();
        assert!(bits & UNDERVOLTAGE_NOW != 0);
        assert!(bits & THROTTLED_NOW != 0);
        assert!(bits & UNDERVOLTAGE_PAST != 0);
        assert!(bits & SOFT_TEMP_LIMIT_NOW == 0);
        assert_eq!(parse_throttled("garbage"), None);
    }

    fn fake_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("pi-monitor-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sys/class/thermal/thermal_zone0")).unwrap();
        std::fs::create_dir_all(root.join("sys/class/hwmon/hwmon3")).unwrap();
        root
    }

    #[test]
    fn reads_temperature_and_hwmon_from_a_fake_sysfs() {
        let root = fake_root("sysfs");
        std::fs::write(root.join("sys/class/thermal/thermal_zone0/temp"), "61250\n").unwrap();
        std::fs::write(root.join("sys/class/hwmon/hwmon3/name"), "rpi_volt\n").unwrap();
        std::fs::write(root.join("sys/class/hwmon/hwmon3/in0_lcrit_alarm"), "1\n").unwrap();

        let s = Sensors::with_root(&root);
        assert_eq!(s.read_temperature(), Some(61.25));
        assert_eq!(hwmon_undervoltage(&root.join("sys/class/hwmon")), Some(true));

        std::fs::write(root.join("sys/class/hwmon/hwmon3/in0_lcrit_alarm"), "0\n").unwrap();
        assert_eq!(hwmon_undervoltage(&root.join("sys/class/hwmon")), Some(false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn converts_the_fan_duty_cycle() {
        assert_eq!(pwm_to_percent(0), 0);
        assert_eq!(pwm_to_percent(128), 50);
        assert_eq!(pwm_to_percent(255), 100);
        assert_eq!(pwm_to_percent(400), 100);
    }

    #[test]
    fn reads_the_fan_from_a_fake_sysfs() {
        let root = fake_root("fan");
        let hwmon = root.join("sys/class/hwmon");
        // Another device first: it must be skipped
        std::fs::write(hwmon.join("hwmon3/name"), "rpi_volt\n").unwrap();
        std::fs::create_dir_all(hwmon.join("hwmon4")).unwrap();
        std::fs::write(hwmon.join("hwmon4/name"), "pwmfan\n").unwrap();
        std::fs::write(hwmon.join("hwmon4/fan1_input"), "2450\n").unwrap();
        std::fs::write(hwmon.join("hwmon4/pwm1"), "128\n").unwrap();
        assert_eq!(hwmon_fan(&hwmon), Some((Some(2450), Some(50))));

        // Only one of the two files
        std::fs::remove_file(hwmon.join("hwmon4/fan1_input")).unwrap();
        assert_eq!(hwmon_fan(&hwmon), Some((None, Some(50))));
        std::fs::remove_file(hwmon.join("hwmon4/pwm1")).unwrap();
        assert_eq!(hwmon_fan(&hwmon), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_sensors_are_none() {
        let root = std::env::temp_dir().join("pi-monitor-test-empty-does-not-exist");
        let s = Sensors::with_root(&root);
        assert_eq!(s.read_temperature(), None);
        assert_eq!(hwmon_undervoltage(&root.join("sys/class/hwmon")), None);
        assert_eq!(hwmon_fan(&root.join("sys/class/hwmon")), None);
        assert_eq!(board_model(&root), "");
    }
}

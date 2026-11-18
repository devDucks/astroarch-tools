//! Raspberry Pi sensors: SoC temperature, supply voltage, firmware throttling flags; on
//! another computer (x86), the CPU temperature and a fan, from hwmon.
//!
//! - temperature: the CPU's hwmon device (`cpu_thermal` on a Pi, `coretemp` on Intel,
//!   `k10temp` or `zenpower` on AMD), else `/sys/class/thermal` (`x86_pkg_temp`, then
//!   `thermal_zone0`), in millidegrees Celsius
//! - Pi 5: the 5 V input rail, `vcgencmd pmic_read_adc EXT5V_V`
//! - Pi 4: no input voltage sensor; the core voltage (`vcgencmd measure_volts core`) is
//!   reported instead, and under-voltage is known from the firmware flag only
//! - fan (Pi 5, official Active Cooler): the `pwmfan` hwmon device, `fan1_input` (rpm) and
//!   `pwm1` (duty cycle, 0-255); a Pi 4 has no fan sensor. Elsewhere the first hwmon fan
//!   that turns, speed only
//! - `vcgencmd` is only run on a Raspberry Pi (it comes with the package raspberrypi-utils,
//!   which also lets the `video` group use /dev/vcio)
//! - under-voltage now / since boot, throttling: `vcgencmd get_throttled`, and the kernel's
//!   `rpi_volt` hwmon device (`in0_lcrit_alarm`) for "under-voltage now" without vcgencmd

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// The kind of machine, from the device tree
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    /// BCM2712: Raspberry Pi 5, 500, CM5. The PMIC measures the 5 V input
    Pi5,
    /// BCM2711: Raspberry Pi 4, 400, CM4. No input voltage sensor
    Pi4,
    /// An older Raspberry Pi
    OtherPi,
    /// Not a Raspberry Pi (x86...): no vcgencmd, temperature and fan from hwmon
    Generic,
}

impl Board {
    /// From `proc/device-tree/compatible` under `root` ("raspberrypi,5-model-b\0brcm,bcm2712")
    pub fn detect(root: &Path) -> Board {
        let compatible = std::fs::read(root.join("proc/device-tree/compatible")).unwrap_or_default();
        let compatible = String::from_utf8_lossy(&compatible);
        let entries: Vec<&str> = compatible.split('\0').collect();
        if entries.contains(&"brcm,bcm2712") {
            Board::Pi5
        } else if entries.contains(&"brcm,bcm2711") {
            Board::Pi4
        } else if entries.iter().any(|c| c.starts_with("raspberrypi,")) {
            Board::OtherPi
        } else {
            Board::Generic
        }
    }

    /// This machine's (read once)
    pub fn this() -> Board {
        static BOARD: OnceLock<Board> = OnceLock::new();
        *BOARD.get_or_init(|| Board::detect(Path::new("/")))
    }

    pub fn is_pi(self) -> bool {
        self != Board::Generic
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Board::Pi5 => "pi5",
            Board::Pi4 => "pi4",
            Board::OtherPi => "pi",
            Board::Generic => "generic",
        }
    }
}

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

/// The hwmon devices, sorted (hwmon0, hwmon1...: the same order at every sample)
fn hwmon_dirs(hwmon_root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(hwmon_root) {
        Ok(entries) => entries.flatten().map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    };
    dirs.sort();
    dirs
}

fn hwmon_name(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("name")).map(|n| n.trim().to_string()).unwrap_or_default()
}

/// The CPU temperature from hwmon: the first device found among the known CPU sensors, in
/// this order; for each, the input with one of the preferred labels, else `temp1_input`
fn hwmon_cpu_temperature(hwmon_root: &Path) -> Option<f64> {
    const CPU_SENSORS: [(&str, &[&str]); 4] = [
        ("cpu_thermal", &[]),
        ("coretemp", &["Package id 0"]),
        ("k10temp", &["Tdie", "Tctl"]),
        ("zenpower", &["Tdie", "Tctl"]),
    ];
    let dirs = hwmon_dirs(hwmon_root);
    for (name, labels) in CPU_SENSORS {
        for dir in dirs.iter().filter(|d| hwmon_name(d) == name) {
            let labelled = labels.iter().find_map(|wanted| {
                (1..=16).find_map(|i| {
                    let label = std::fs::read_to_string(dir.join(format!("temp{i}_label"))).ok()?;
                    (label.trim() == *wanted).then(|| dir.join(format!("temp{i}_input")))
                })
            });
            let input = labelled.unwrap_or_else(|| dir.join("temp1_input"));
            if let Some(v) = std::fs::read_to_string(input).ok().and_then(|s| parse_millidegrees(&s)) {
                return Some(v);
            }
        }
    }
    None
}

/// The CPU temperature from /sys/class/thermal: the `x86_pkg_temp` zone (Intel), else zone 0
fn thermal_zone_temperature(thermal_root: &Path) -> Option<f64> {
    let mut zones: Vec<PathBuf> = std::fs::read_dir(thermal_root)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    zones.sort();
    let package = zones.iter().find(|z| {
        std::fs::read_to_string(z.join("type")).is_ok_and(|t| t.trim() == "x86_pkg_temp")
    });
    let zone = package.cloned().unwrap_or_else(|| thermal_root.join("thermal_zone0"));
    parse_millidegrees(&std::fs::read_to_string(zone.join("temp")).ok()?)
}

/// The fan: the `pwmfan` hwmon device (Pi 5 Active Cooler), (rpm, percent) whichever could
/// be read; else the first hwmon fan that turns (x86: speed only, the duty cycle of a
/// motherboard's fan header says nothing reliable)
fn hwmon_fan(hwmon_root: &Path) -> Option<(Option<u32>, Option<u32>)> {
    if let Some(found) = pwmfan(hwmon_root) {
        return Some(found);
    }
    for dir in hwmon_dirs(hwmon_root) {
        for i in 1..=8 {
            let rpm = std::fs::read_to_string(dir.join(format!("fan{i}_input")))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok());
            // 0 rpm: an empty fan header, or a fan that is off: not shown
            if let Some(rpm) = rpm.filter(|&r| r > 0) {
                return Some((Some(rpm), None));
            }
        }
    }
    None
}

fn pwmfan(hwmon_root: &Path) -> Option<(Option<u32>, Option<u32>)> {
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

/// After this many failed `pmic_read_adc` in a row, stop asking (no vcgencmd, no access
/// to /dev/vcio): not a process for nothing at every sample. One failure is not enough, a
/// call can time out once
const PMIC_TRIES: u32 = 3;

/// Where the sensors are read; `root` is `/` except in tests
pub struct Sensors {
    root: PathBuf,
    board: Board,
    /// Failed `pmic_read_adc` in a row (see PMIC_TRIES)
    pmic_failures: u32,
}

impl Default for Sensors {
    fn default() -> Self {
        Self::with_root("/")
    }
}

impl Sensors {
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self { board: Board::detect(&root), root, pmic_failures: 0 }
    }

    pub fn board(&self) -> Board {
        self.board
    }

    fn read_temperature(&self) -> Option<f64> {
        hwmon_cpu_temperature(&self.root.join("sys/class/hwmon"))
            .or_else(|| thermal_zone_temperature(&self.root.join("sys/class/thermal")))
    }

    pub async fn read(&mut self) -> Reading {
        let mut r = Reading { temperature: self.read_temperature(), ..Reading::default() };

        // The firmware's values: a Raspberry Pi only
        if self.board == Board::Pi5 && self.pmic_failures < PMIC_TRIES {
            match vcgencmd(&["pmic_read_adc", "EXT5V_V"]).await.and_then(|s| parse_volts(&s)) {
                Some(v) => {
                    r.voltage = Some(v);
                    r.voltage_kind = VoltageKind::Supply5V;
                    self.pmic_failures = 0;
                }
                None => self.pmic_failures += 1,
            }
        }
        // Not on a Pi 5 whose PMIC cannot be read: that is no vcgencmd at all, and the core
        // voltage is not what the thresholds are about
        if self.board.is_pi() && self.board != Board::Pi5 {
            if let Some(v) = vcgencmd(&["measure_volts", "core"]).await.and_then(|s| parse_volts(&s)) {
                r.voltage = Some(v);
                r.voltage_kind = VoltageKind::Core;
            }
        }

        if self.board.is_pi() {
            if let Some(bits) = vcgencmd(&["get_throttled"]).await.and_then(|s| parse_throttled(&s)) {
                r.undervoltage_now = bits & UNDERVOLTAGE_NOW != 0;
                r.undervoltage_past = bits & UNDERVOLTAGE_PAST != 0;
                r.throttled_now = bits & THROTTLED_NOW != 0;
                r.soft_temp_limit = bits & SOFT_TEMP_LIMIT_NOW != 0;
            }
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

/// The board's model, e.g. "Raspberry Pi 5 Model B Rev 1.0"; on a PC the vendor and the
/// product from the firmware (DMI), e.g. "ASUSTeK COMPUTER INC. PN64"; empty if unknown
pub fn board_model(root: &Path) -> String {
    let read = |rel: &str| {
        std::fs::read_to_string(root.join(rel))
            .map(|s| s.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string())
            .unwrap_or_default()
    };
    let model = read("proc/device-tree/model");
    if !model.is_empty() {
        return model;
    }
    // Placeholders some firmwares leave in the DMI fields
    let useful = |s: &String| {
        let l = s.to_ascii_lowercase();
        !s.is_empty() && !l.contains("o.e.m") && !l.contains("default string") && !l.contains("not specified")
    };
    [read("sys/class/dmi/id/sys_vendor"), read("sys/class/dmi/id/product_name")]
        .into_iter()
        .filter(useful)
        .collect::<Vec<_>>()
        .join(" ")
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
    fn detects_the_board_from_the_device_tree() {
        let root = fake_root("board");
        assert_eq!(Board::detect(&root), Board::Generic);
        std::fs::create_dir_all(root.join("proc/device-tree")).unwrap();
        let compatible = root.join("proc/device-tree/compatible");
        std::fs::write(&compatible, "raspberrypi,5-model-b\0brcm,bcm2712\0").unwrap();
        assert_eq!(Board::detect(&root), Board::Pi5);
        std::fs::write(&compatible, "raspberrypi,500\0brcm,bcm2712\0").unwrap();
        assert_eq!(Board::detect(&root), Board::Pi5);
        std::fs::write(&compatible, "raspberrypi,4-model-b\0brcm,bcm2711\0").unwrap();
        assert_eq!(Board::detect(&root), Board::Pi4);
        std::fs::write(&compatible, "raspberrypi,3-model-b-plus\0brcm,bcm2837\0").unwrap();
        assert_eq!(Board::detect(&root), Board::OtherPi);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn hwmon_device(root: &Path, n: u32, name: &str, files: &[(&str, &str)]) {
        let dir = root.join(format!("sys/class/hwmon/hwmon{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("name"), format!("{name}\n")).unwrap();
        for (file, content) in files {
            std::fs::write(dir.join(file), format!("{content}\n")).unwrap();
        }
    }

    #[test]
    fn reads_the_cpu_temperature_of_an_amd_pc() {
        // As on a Ryzen mini PC: no thermal zone, the disks first, then k10temp
        let root = fake_root("amd");
        let _ = std::fs::remove_dir_all(root.join("sys/class/thermal"));
        hwmon_device(&root, 0, "nvme", &[("temp1_input", "41850"), ("temp1_label", "Composite")]);
        hwmon_device(&root, 7, "k10temp", &[("temp1_input", "52125"), ("temp1_label", "Tctl")]);
        assert_eq!(Sensors::with_root(&root).read_temperature(), Some(52.125));
        // A Tdie is preferred to the Tctl (which can carry an offset)
        hwmon_device(&root, 7, "k10temp", &[("temp2_input", "42125"), ("temp2_label", "Tdie")]);
        assert_eq!(Sensors::with_root(&root).read_temperature(), Some(42.125));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_the_package_temperature_of_an_intel_pc() {
        let root = fake_root("intel");
        hwmon_device(&root, 2, "acpitz", &[("temp1_input", "27800")]);
        hwmon_device(
            &root,
            4,
            "coretemp",
            &[("temp1_input", "61000"), ("temp1_label", "Package id 0"), ("temp2_input", "58000"), ("temp2_label", "Core 0")],
        );
        assert_eq!(Sensors::with_root(&root).read_temperature(), Some(61.0));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn falls_back_to_the_x86_package_thermal_zone() {
        let root = fake_root("zone");
        std::fs::write(root.join("sys/class/thermal/thermal_zone0/type"), "acpitz\n").unwrap();
        std::fs::write(root.join("sys/class/thermal/thermal_zone0/temp"), "27800\n").unwrap();
        let zone = root.join("sys/class/thermal/thermal_zone3");
        std::fs::create_dir_all(&zone).unwrap();
        std::fs::write(zone.join("type"), "x86_pkg_temp\n").unwrap();
        std::fs::write(zone.join("temp"), "64000\n").unwrap();
        assert_eq!(Sensors::with_root(&root).read_temperature(), Some(64.0));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_a_turning_pc_fan_without_a_duty_cycle() {
        let root = fake_root("pcfan");
        let hwmon = root.join("sys/class/hwmon");
        hwmon_device(&root, 5, "nct6798", &[("fan1_input", "0"), ("fan2_input", "1130"), ("pwm2", "120")]);
        assert_eq!(hwmon_fan(&hwmon), Some((Some(1130), None)));
        // Only empty headers: no fan shown
        hwmon_device(&root, 5, "nct6798", &[("fan2_input", "0")]);
        assert_eq!(hwmon_fan(&hwmon), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_model_of_a_pc_comes_from_dmi() {
        let root = fake_root("dmi");
        let dmi = root.join("sys/class/dmi/id");
        std::fs::create_dir_all(&dmi).unwrap();
        std::fs::write(dmi.join("sys_vendor"), "To Be Filled By O.E.M.\n").unwrap();
        std::fs::write(dmi.join("product_name"), "WTR MAX\n").unwrap();
        assert_eq!(board_model(&root), "WTR MAX");
        std::fs::write(dmi.join("sys_vendor"), "ASUSTeK COMPUTER INC.\n").unwrap();
        assert_eq!(board_model(&root), "ASUSTeK COMPUTER INC. WTR MAX");
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

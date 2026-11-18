//! Thresholds, alert levels (with hysteresis) and when a notification is due.
//! Pure logic, no I/O: everything here is unit tested.

use crate::sensors::{Board, Reading, VoltageKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A temperature alert ends once the value is this much under its threshold (degrees)
pub const TEMP_HYSTERESIS: f64 = 2.0;
/// A voltage alert ends once the value is this much over its threshold (volts)
pub const VOLT_HYSTERESIS: f64 = 0.05;
/// A fan that is asked to turn and reads 0 rpm for this many samples in a row is not
/// turning (it needs a moment to spin up)
pub const FAN_STALL_SAMPLES: u32 = 5;
/// After a failed notification (no notification server yet), try again after this delay
const NOTIFY_RETRY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Level {
    #[default]
    Ok = 0,
    Warning = 1,
    Critical = 2,
}

/// The defaults follow the Raspberry Pi firmware: it throttles from 80 °C, and flags
/// under-voltage when the 5 V input drops under about 4.63 V. A missing value in the
/// settings file gets this machine's default (see `for_board`)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default = "Thresholds::for_this_board")]
pub struct Thresholds {
    /// °C
    pub temp_warn: f64,
    pub temp_crit: f64,
    /// V, on the 5 V input (Raspberry Pi 5): the alert is a *low* voltage
    pub volt_warn: f64,
    pub volt_crit: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { temp_warn: 70.0, temp_crit: 80.0, volt_warn: 4.75, volt_crit: 4.63 }
    }
}

impl Thresholds {
    /// The defaults of a kind of machine. An x86 CPU runs hot under load (a Ryzen 7 8845HS
    /// mini PC: 87 °C during a compile) and throttles itself at 100 °C: the Raspberry Pi's
    /// 70/80 °C would warn all the time
    pub fn for_board(board: Board) -> Thresholds {
        match board {
            Board::Generic => Thresholds { temp_warn: 90.0, temp_crit: 98.0, ..Thresholds::default() },
            _ => Thresholds::default(),
        }
    }

    /// The defaults of this machine
    pub fn for_this_board() -> Thresholds {
        Thresholds::for_board(Board::this())
    }

    pub fn validate(&self) -> Result<(), String> {
        let all = [self.temp_warn, self.temp_crit, self.volt_warn, self.volt_crit];
        if all.iter().any(|v| !v.is_finite()) {
            return Err("thresholds must be numbers".into());
        }
        if !(30.0..=110.0).contains(&self.temp_warn) || !(30.0..=110.0).contains(&self.temp_crit) {
            return Err("temperature thresholds must be between 30 and 110 °C".into());
        }
        if self.temp_warn >= self.temp_crit {
            return Err("the temperature warning must be under the critical threshold".into());
        }
        if !(3.0..=5.5).contains(&self.volt_warn) || !(3.0..=5.5).contains(&self.volt_crit) {
            return Err("voltage thresholds must be between 3.0 and 5.5 V".into());
        }
        if self.volt_crit >= self.volt_warn {
            return Err("the voltage critical threshold must be under the warning one".into());
        }
        Ok(())
    }
}

/// One active alert
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Alert {
    Temperature { level: Level, value: f64 },
    Voltage { level: Level, value: f64 },
    /// The firmware reports an under-voltage right now
    Undervoltage,
    /// The firmware reports that the CPU is being throttled right now
    Throttled,
    /// The fan is asked to turn and does not: unplugged, blocked or dead
    FanStalled,
}

impl Alert {
    pub fn level(&self) -> Level {
        match self {
            Alert::Temperature { level, .. } | Alert::Voltage { level, .. } => *level,
            Alert::Undervoltage => Level::Critical,
            Alert::Throttled | Alert::FanStalled => Level::Warning,
        }
    }

    /// The kind of alert, whatever its level and value
    pub fn kind(&self) -> u8 {
        match self {
            Alert::Temperature { .. } => 0,
            Alert::Voltage { .. } => 1,
            Alert::Undervoltage => 2,
            Alert::Throttled => 3,
            Alert::FanStalled => 4,
        }
    }

    /// What identifies an alert for "did it change?": its kind and its level (the value
    /// changes at every sample)
    pub fn key(&self) -> (u8, Level) {
        (self.kind(), self.level())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Evaluation {
    pub level: Level,
    pub alerts: Vec<Alert>,
}

/// Turns readings into alerts; remembers the previous levels for the hysteresis, so a
/// value hovering around a threshold does not make the alert flap
#[derive(Debug, Default)]
pub struct Evaluator {
    temp: Level,
    volt: Level,
    /// Samples in a row where the fan was asked to turn and did not
    stalled: u32,
}

/// A value where high is bad (temperature)
fn high_is_bad(prev: Level, v: f64, warn: f64, crit: f64, hysteresis: f64) -> Level {
    let now = if v >= crit {
        Level::Critical
    } else if v >= warn {
        Level::Warning
    } else {
        Level::Ok
    };
    let held = if prev == Level::Critical && v >= crit - hysteresis {
        Level::Critical
    } else if prev >= Level::Warning && v >= warn - hysteresis {
        Level::Warning
    } else {
        Level::Ok
    };
    now.max(held)
}

/// A value where low is bad (supply voltage)
fn low_is_bad(prev: Level, v: f64, warn: f64, crit: f64, hysteresis: f64) -> Level {
    let now = if v <= crit {
        Level::Critical
    } else if v <= warn {
        Level::Warning
    } else {
        Level::Ok
    };
    let held = if prev == Level::Critical && v <= crit + hysteresis {
        Level::Critical
    } else if prev >= Level::Warning && v <= warn + hysteresis {
        Level::Warning
    } else {
        Level::Ok
    };
    now.max(held)
}

impl Evaluator {
    pub fn evaluate(&mut self, r: &Reading, t: &Thresholds) -> Evaluation {
        let mut alerts = Vec::new();

        match r.temperature {
            Some(v) => {
                self.temp = high_is_bad(self.temp, v, t.temp_warn, t.temp_crit, TEMP_HYSTERESIS);
                if self.temp > Level::Ok {
                    alerts.push(Alert::Temperature { level: self.temp, value: v });
                }
            }
            None => self.temp = Level::Ok,
        }

        // The thresholds are for the 5 V input; the core voltage of a Pi 4 is not comparable
        match (r.voltage, r.voltage_kind) {
            (Some(v), VoltageKind::Supply5V) => {
                self.volt = low_is_bad(self.volt, v, t.volt_warn, t.volt_crit, VOLT_HYSTERESIS);
                if self.volt > Level::Ok {
                    alerts.push(Alert::Voltage { level: self.volt, value: v });
                }
            }
            _ => self.volt = Level::Ok,
        }

        if r.fan_percent.is_some_and(|p| p > 0) && r.fan_rpm == Some(0) {
            self.stalled = self.stalled.saturating_add(1);
        } else {
            self.stalled = 0;
        }
        if self.stalled >= FAN_STALL_SAMPLES {
            alerts.push(Alert::FanStalled);
        }

        if r.undervoltage_now {
            alerts.push(Alert::Undervoltage);
        }
        if r.throttled_now {
            alerts.push(Alert::Throttled);
        }

        let level = alerts.iter().map(Alert::level).max().unwrap_or(Level::Ok);
        Evaluation { level, alerts }
    }
}

/// Decides when a notification is sent: when an alert appears or gets worse, and again
/// every `repeat` while something is critical. Nothing is sent for an alert that was
/// already notified at that level or higher (a critical alert going back to a warning is
/// not news)
#[derive(Debug, Default)]
pub struct NotifyPolicy {
    /// The highest level notified for each kind of alert, while it stays active
    known: HashMap<u8, Level>,
    last: Option<Instant>,
    retry_at: Option<Instant>,
}

impl NotifyPolicy {
    pub fn due(&mut self, alerts: &[Alert], now: Instant, repeat: Option<Duration>) -> bool {
        // An alert that is gone will be notified again if it comes back
        self.known.retain(|kind, _| alerts.iter().any(|a| a.kind() == *kind));
        if alerts.is_empty() {
            self.last = None;
            self.retry_at = None;
            return false;
        }
        if self.retry_at.is_some_and(|t| now < t) {
            return false;
        }
        let is_new = alerts.iter().any(|a| self.known.get(&a.kind()).is_none_or(|known| a.level() > *known));
        let repeat_due = alerts.iter().any(|a| a.level() == Level::Critical)
            && repeat
                .zip(self.last)
                .is_some_and(|(every, last)| now.duration_since(last) >= every);
        is_new || repeat_due
    }

    /// The notification was delivered
    pub fn sent(&mut self, alerts: &[Alert], now: Instant) {
        for a in alerts {
            let known = self.known.entry(a.kind()).or_insert(a.level());
            *known = (*known).max(a.level());
        }
        self.last = Some(now);
        self.retry_at = None;
    }

    /// The notification could not be delivered: try again later, not at every sample
    pub fn failed(&mut self, now: Instant) {
        self.retry_at = Some(now + NOTIFY_RETRY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(temp: f64, volt: f64) -> Reading {
        Reading {
            temperature: Some(temp),
            voltage: Some(volt),
            voltage_kind: VoltageKind::Supply5V,
            ..Reading::default()
        }
    }

    #[test]
    fn default_thresholds_are_valid() {
        assert!(Thresholds::default().validate().is_ok());
    }

    #[test]
    fn a_pc_gets_higher_temperature_defaults() {
        assert_eq!(Thresholds::for_board(Board::Pi5), Thresholds::default());
        assert_eq!(Thresholds::for_board(Board::Pi4), Thresholds::default());
        let pc = Thresholds::for_board(Board::Generic);
        assert_eq!((pc.temp_warn, pc.temp_crit), (90.0, 98.0));
        assert!(pc.validate().is_ok());
    }

    #[test]
    fn rejects_inverted_or_absurd_thresholds() {
        let t = Thresholds { temp_warn: 80.0, temp_crit: 70.0, ..Thresholds::default() };
        assert!(t.validate().is_err());
        let t = Thresholds { volt_warn: 4.6, volt_crit: 4.7, ..Thresholds::default() };
        assert!(t.validate().is_err());
        let t = Thresholds { temp_crit: 200.0, ..Thresholds::default() };
        assert!(t.validate().is_err());
        let t = Thresholds { volt_crit: f64::NAN, ..Thresholds::default() };
        assert!(t.validate().is_err());
    }

    #[test]
    fn levels_follow_the_thresholds() {
        let t = Thresholds::default();
        let mut e = Evaluator::default();
        assert_eq!(e.evaluate(&reading(50.0, 5.1), &t).level, Level::Ok);
        assert_eq!(e.evaluate(&reading(72.0, 5.1), &t).level, Level::Warning);
        assert_eq!(e.evaluate(&reading(81.0, 5.1), &t).level, Level::Critical);
        let mut e = Evaluator::default();
        assert_eq!(e.evaluate(&reading(50.0, 4.70), &t).level, Level::Warning);
        assert_eq!(e.evaluate(&reading(50.0, 4.60), &t).level, Level::Critical);
    }

    #[test]
    fn temperature_hysteresis_prevents_flapping() {
        let t = Thresholds::default();
        let mut e = Evaluator::default();
        assert_eq!(e.evaluate(&reading(80.5, 5.1), &t).level, Level::Critical);
        // Just under 80 °C: still critical (within the 2 °C hysteresis)
        assert_eq!(e.evaluate(&reading(79.0, 5.1), &t).level, Level::Critical);
        // Under 78 °C: drops to the warning level
        assert_eq!(e.evaluate(&reading(77.0, 5.1), &t).level, Level::Warning);
        // Just under 70 °C: still a warning
        assert_eq!(e.evaluate(&reading(69.0, 5.1), &t).level, Level::Warning);
        assert_eq!(e.evaluate(&reading(67.0, 5.1), &t).level, Level::Ok);
    }

    #[test]
    fn voltage_hysteresis_prevents_flapping() {
        let t = Thresholds::default();
        let mut e = Evaluator::default();
        assert_eq!(e.evaluate(&reading(50.0, 4.60), &t).level, Level::Critical);
        assert_eq!(e.evaluate(&reading(50.0, 4.66), &t).level, Level::Critical);
        assert_eq!(e.evaluate(&reading(50.0, 4.72), &t).level, Level::Warning);
        assert_eq!(e.evaluate(&reading(50.0, 4.78), &t).level, Level::Warning);
        assert_eq!(e.evaluate(&reading(50.0, 4.85), &t).level, Level::Ok);
    }

    #[test]
    fn core_voltage_is_never_compared_to_the_supply_thresholds() {
        let t = Thresholds::default();
        let mut e = Evaluator::default();
        let r = Reading {
            temperature: Some(40.0),
            voltage: Some(0.86),
            voltage_kind: VoltageKind::Core,
            ..Reading::default()
        };
        assert_eq!(e.evaluate(&r, &t).level, Level::Ok);
    }

    #[test]
    fn firmware_flags_raise_alerts() {
        let t = Thresholds::default();
        let mut e = Evaluator::default();
        let r = Reading { undervoltage_now: true, throttled_now: true, ..Reading::default() };
        let ev = e.evaluate(&r, &t);
        assert_eq!(ev.level, Level::Critical);
        assert!(ev.alerts.contains(&Alert::Undervoltage));
        assert!(ev.alerts.contains(&Alert::Throttled));
        // "since boot" is history, not an alert
        let r = Reading { undervoltage_past: true, ..Reading::default() };
        assert_eq!(e.evaluate(&r, &t).level, Level::Ok);
    }

    fn fan(rpm: u32, percent: u32) -> Reading {
        Reading { fan_rpm: Some(rpm), fan_percent: Some(percent), ..Reading::default() }
    }

    #[test]
    fn a_fan_that_does_not_turn_raises_an_alert_after_a_few_samples() {
        let t = Thresholds::default();
        let mut e = Evaluator::default();
        for _ in 0..FAN_STALL_SAMPLES - 1 {
            assert_eq!(e.evaluate(&fan(0, 60), &t).level, Level::Ok);
        }
        let ev = e.evaluate(&fan(0, 60), &t);
        assert_eq!(ev.alerts, vec![Alert::FanStalled]);
        assert_eq!(ev.level, Level::Warning);
        // It turns again: the alert goes away
        assert_eq!(e.evaluate(&fan(2400, 60), &t).level, Level::Ok);
    }

    #[test]
    fn a_fan_that_is_not_asked_to_turn_is_fine() {
        let t = Thresholds::default();
        let mut e = Evaluator::default();
        for _ in 0..FAN_STALL_SAMPLES * 2 {
            assert_eq!(e.evaluate(&fan(0, 0), &t).level, Level::Ok);
        }
        // No fan at all (Pi 4)
        for _ in 0..FAN_STALL_SAMPLES * 2 {
            assert_eq!(e.evaluate(&Reading::default(), &t).level, Level::Ok);
        }
    }

    #[test]
    fn a_stall_interrupted_by_a_turn_starts_over() {
        let t = Thresholds::default();
        let mut e = Evaluator::default();
        for _ in 0..FAN_STALL_SAMPLES - 1 {
            e.evaluate(&fan(0, 60), &t);
        }
        e.evaluate(&fan(2400, 60), &t);
        for _ in 0..FAN_STALL_SAMPLES - 1 {
            assert_eq!(e.evaluate(&fan(0, 60), &t).level, Level::Ok);
        }
    }

    #[test]
    fn missing_sensors_make_no_alert() {
        let mut e = Evaluator::default();
        assert_eq!(e.evaluate(&Reading::default(), &Thresholds::default()), Evaluation::default());
    }

    fn temp_alert(level: Level) -> Alert {
        Alert::Temperature { level, value: 75.0 }
    }

    #[test]
    fn notifies_once_per_new_alert() {
        let mut p = NotifyPolicy::default();
        let t0 = Instant::now();
        let alerts = [temp_alert(Level::Warning)];
        assert!(p.due(&alerts, t0, None));
        p.sent(&alerts, t0);
        // Same alert, a few samples later: nothing
        assert!(!p.due(&alerts, t0 + Duration::from_secs(2), None));
        // It gets worse: new notification
        let worse = [temp_alert(Level::Critical)];
        assert!(p.due(&worse, t0 + Duration::from_secs(4), None));
        p.sent(&worse, t0 + Duration::from_secs(4));
        // A second, different alert appears
        let two = [temp_alert(Level::Critical), Alert::Undervoltage];
        assert!(p.due(&two, t0 + Duration::from_secs(6), None));
    }

    #[test]
    fn an_alert_that_gets_better_is_not_notified() {
        let mut p = NotifyPolicy::default();
        let t0 = Instant::now();
        let crit = [temp_alert(Level::Critical)];
        assert!(p.due(&crit, t0, None));
        p.sent(&crit, t0);
        // Critical -> warning: not news
        let warn = [temp_alert(Level::Warning)];
        assert!(!p.due(&warn, t0 + Duration::from_secs(2), None));
        // And back to critical while it never went away: already notified
        assert!(!p.due(&crit, t0 + Duration::from_secs(4), None));
        // Another kind of alert at the warning level is new
        let two = [temp_alert(Level::Warning), Alert::FanStalled];
        assert!(p.due(&two, t0 + Duration::from_secs(6), None));
    }

    #[test]
    fn notifies_again_after_the_alert_went_away() {
        let mut p = NotifyPolicy::default();
        let t0 = Instant::now();
        let alerts = [temp_alert(Level::Warning)];
        assert!(p.due(&alerts, t0, None));
        p.sent(&alerts, t0);
        assert!(!p.due(&[], t0 + Duration::from_secs(2), None));
        assert!(p.due(&alerts, t0 + Duration::from_secs(4), None));
    }

    #[test]
    fn repeats_only_critical_alerts() {
        let mut p = NotifyPolicy::default();
        let t0 = Instant::now();
        let every = Some(Duration::from_secs(600));

        let warn = [temp_alert(Level::Warning)];
        p.sent(&warn, t0);
        assert!(!p.due(&warn, t0 + Duration::from_secs(3600), every));

        let crit = [temp_alert(Level::Critical)];
        p.sent(&crit, t0);
        assert!(!p.due(&crit, t0 + Duration::from_secs(599), every));
        assert!(p.due(&crit, t0 + Duration::from_secs(600), every));
        // Repeat switched off
        assert!(!p.due(&crit, t0 + Duration::from_secs(6000), None));
    }

    #[test]
    fn a_failed_notification_is_retried_later_not_at_every_sample() {
        let mut p = NotifyPolicy::default();
        let t0 = Instant::now();
        let alerts = [temp_alert(Level::Critical)];
        assert!(p.due(&alerts, t0, None));
        p.failed(t0);
        assert!(!p.due(&alerts, t0 + Duration::from_secs(2), None));
        assert!(p.due(&alerts, t0 + Duration::from_secs(31), None));
    }
}

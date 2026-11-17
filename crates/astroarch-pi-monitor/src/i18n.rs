//! The texts of the alerts and notifications, in the languages of AstroArch.
//! The daemon has no Qt, so they are a small table here; the language is the session's
//! (`LC_ALL`, `LC_MESSAGES`, `LANGUAGE`, `LANG`, then `/etc/locale.conf`).

use crate::alerts::{Alert, Level};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
    De,
    It,
    Es,
}

impl Lang {
    /// "fr_FR.UTF-8", "fr", "fr:en" -> Fr; anything unknown -> En
    pub fn from_tag(tag: &str) -> Lang {
        match tag.trim().to_ascii_lowercase().get(..2) {
            Some("fr") => Lang::Fr,
            Some("de") => Lang::De,
            Some("it") => Lang::It,
            Some("es") => Lang::Es,
            _ => Lang::En,
        }
    }

    pub fn detect() -> Lang {
        for var in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
            if let Ok(v) = std::env::var(var) {
                if !v.is_empty() && v != "C" && v != "POSIX" {
                    return Lang::from_tag(&v);
                }
            }
        }
        // A systemd user service does not always get the session's locale
        if let Ok(conf) = std::fs::read_to_string("/etc/locale.conf") {
            for line in conf.lines() {
                if let Some(v) = line.trim().strip_prefix("LANG=") {
                    return Lang::from_tag(v.trim_matches('"'));
                }
            }
        }
        Lang::En
    }
}

pub fn alert_text(lang: Lang, alert: &Alert) -> String {
    use Lang::*;
    match *alert {
        Alert::Temperature { level, value } => {
            let critical = level == Level::Critical;
            let v = format!("{value:.1}");
            match (lang, critical) {
                (En, false) => format!("CPU temperature high: {v} °C"),
                (En, true) => format!("CPU temperature critical: {v} °C"),
                (Fr, false) => format!("Température du processeur élevée : {v} °C"),
                (Fr, true) => format!("Température du processeur critique : {v} °C"),
                (De, false) => format!("CPU-Temperatur hoch: {v} °C"),
                (De, true) => format!("CPU-Temperatur kritisch: {v} °C"),
                (It, false) => format!("Temperatura della CPU alta: {v} °C"),
                (It, true) => format!("Temperatura della CPU critica: {v} °C"),
                (Es, false) => format!("Temperatura de la CPU alta: {v} °C"),
                (Es, true) => format!("Temperatura de la CPU crítica: {v} °C"),
            }
        }
        Alert::Voltage { level, value } => {
            let critical = level == Level::Critical;
            let v = format!("{value:.2}");
            match (lang, critical) {
                (En, false) => format!("Supply voltage low: {v} V"),
                (En, true) => format!("Supply voltage critical: {v} V"),
                (Fr, false) => format!("Tension d'alimentation basse : {v} V"),
                (Fr, true) => format!("Tension d'alimentation critique : {v} V"),
                (De, false) => format!("Versorgungsspannung niedrig: {v} V"),
                (De, true) => format!("Versorgungsspannung kritisch: {v} V"),
                (It, false) => format!("Tensione di alimentazione bassa: {v} V"),
                (It, true) => format!("Tensione di alimentazione critica: {v} V"),
                (Es, false) => format!("Tensión de alimentación baja: {v} V"),
                (Es, true) => format!("Tensión de alimentación crítica: {v} V"),
            }
        }
        Alert::Undervoltage => match lang {
            En => "Under-voltage detected: check the power supply",
            Fr => "Sous-tension détectée : vérifiez l'alimentation",
            De => "Unterspannung erkannt: Netzteil prüfen",
            It => "Sottotensione rilevata: controlla l'alimentatore",
            Es => "Subtensión detectada: revisa la fuente de alimentación",
        }
        .to_string(),
        Alert::FanStalled => match lang {
            En => "The fan is not turning: check the fan cable",
            Fr => "Le ventilateur ne tourne pas : vérifiez son câble",
            De => "Der Lüfter dreht sich nicht: Lüfterkabel prüfen",
            It => "La ventola non gira: controlla il cavo della ventola",
            Es => "El ventilador no gira: revisa el cable del ventilador",
        }
        .to_string(),
        Alert::Throttled => match lang {
            En => "The CPU is being throttled",
            Fr => "Le processeur est bridé (throttling)",
            De => "Die CPU wird gedrosselt",
            It => "La CPU è in throttling",
            Es => "La CPU está limitada (throttling)",
        }
        .to_string(),
    }
}

/// The notification's title
pub fn title(lang: Lang, level: Level) -> &'static str {
    use Lang::*;
    match (lang, level == Level::Critical) {
        (En, false) => "Raspberry Pi: warning",
        (En, true) => "Raspberry Pi: critical alert",
        (Fr, false) => "Raspberry Pi : attention",
        (Fr, true) => "Raspberry Pi : alerte critique",
        (De, false) => "Raspberry Pi: Warnung",
        (De, true) => "Raspberry Pi: Kritischer Alarm",
        (It, false) => "Raspberry Pi: attenzione",
        (It, true) => "Raspberry Pi: allarme critico",
        (Es, false) => "Raspberry Pi: aviso",
        (Es, true) => "Raspberry Pi: alerta crítica",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_language_from_a_locale_tag() {
        assert_eq!(Lang::from_tag("fr_FR.UTF-8"), Lang::Fr);
        assert_eq!(Lang::from_tag("de"), Lang::De);
        assert_eq!(Lang::from_tag("it_IT"), Lang::It);
        assert_eq!(Lang::from_tag("es_ES.UTF-8"), Lang::Es);
        assert_eq!(Lang::from_tag("fr:en"), Lang::Fr);
        assert_eq!(Lang::from_tag("C.UTF-8"), Lang::En);
        assert_eq!(Lang::from_tag(""), Lang::En);
    }

    #[test]
    fn texts_carry_the_value() {
        let a = Alert::Temperature { level: Level::Critical, value: 81.26 };
        assert_eq!(alert_text(Lang::En, &a), "CPU temperature critical: 81.3 °C");
        assert!(alert_text(Lang::Fr, &a).contains("81.3"));
        let a = Alert::Voltage { level: Level::Warning, value: 4.7149 };
        assert_eq!(alert_text(Lang::En, &a), "Supply voltage low: 4.71 V");
    }
}

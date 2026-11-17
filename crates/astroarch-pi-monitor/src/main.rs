//! astroarch-pi-monitor: temperature and voltage of the Raspberry Pi, alerts and
//! notifications. One binary:
//!
//!   daemon     the D-Bus service (started by systemd, see `install`)
//!   status     what the service reports (or a direct reading if it is not running)
//!   set        change the thresholds and the notifications
//!   install    put the binary, the Plasma widget and the service in place
//!   uninstall  take them away again

use astroarch_pi_monitor::{client, daemon, install};
use std::process::ExitCode;

const USAGE: &str = "\
astroarch-pi-monitor: Raspberry Pi temperature and voltage, alerts and notifications

Usage:
  astroarch-pi-monitor daemon                 run the D-Bus service
  astroarch-pi-monitor status [--json]        show the temperature, the voltage and the alerts
  astroarch-pi-monitor set [options]          change the settings (the service applies them at once)
      --temp-warn <°C>    temperature warning        (default 70)
      --temp-crit <°C>    temperature critical       (default 80)
      --volt-warn <V>     5 V input warning, under   (default 4.75; Raspberry Pi 5)
      --volt-crit <V>     5 V input critical, under  (default 4.63; Raspberry Pi 5)
      --notifications <on|off>
  astroarch-pi-monitor install [--system]     install the binary, the Plasma widget and the service
  astroarch-pi-monitor uninstall [--system]   remove them (the settings are kept)

The settings are in ~/.config/astroarch/pi-monitor.toml
";

fn parse_number(flag: &str, value: Option<String>) -> Result<f64, String> {
    let v = value.ok_or(format!("{flag} needs a value"))?;
    v.replace(',', ".").parse().map_err(|_| format!("{flag}: \"{v}\" is not a number"))
}

fn parse_set(args: impl Iterator<Item = String>) -> Result<client::Changes, String> {
    let mut args = args;
    let mut c = client::Changes::default();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--temp-warn" => c.temp_warn = Some(parse_number(&flag, args.next())?),
            "--temp-crit" => c.temp_crit = Some(parse_number(&flag, args.next())?),
            "--volt-warn" => c.volt_warn = Some(parse_number(&flag, args.next())?),
            "--volt-crit" => c.volt_crit = Some(parse_number(&flag, args.next())?),
            "--notifications" => {
                c.notifications = match args.next().as_deref() {
                    Some("on" | "true" | "yes" | "1") => Some(true),
                    Some("off" | "false" | "no" | "0") => Some(false),
                    _ => return Err("--notifications needs on or off".into()),
                }
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    if c.is_empty() {
        return Err("nothing to change (see --help)".into());
    }
    Ok(c)
}

async fn run(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    match args.next().as_deref() {
        Some("daemon") => daemon::run().await.map_err(|e| e.to_string()),
        Some("status") => {
            let json = match args.next().as_deref() {
                None => false,
                Some("--json") => true,
                Some(other) => return Err(format!("unknown option {other}")),
            };
            let s = client::status().await;
            if json {
                println!("{}", serde_json::to_string(&s).map_err(|e| e.to_string())?);
            } else {
                print!("{}", s.to_text());
            }
            Ok(())
        }
        Some("set") => {
            let cfg = client::set(parse_set(args)?).await?;
            let t = cfg.thresholds;
            println!(
                "temperature: warning {} °C, critical {} °C; voltage: warning {} V, critical {} V; notifications {}",
                t.temp_warn,
                t.temp_crit,
                t.volt_warn,
                t.volt_crit,
                if cfg.notifications.enabled { "on" } else { "off" }
            );
            Ok(())
        }
        Some("install") => install::install(args.next().as_deref() == Some("--system")),
        Some("uninstall") => install::uninstall(args.next().as_deref() == Some("--system")),
        Some("-V" | "--version") => {
            println!("astroarch-pi-monitor {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        None | Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other} (see --help)")),
    }
}

// One thread is plenty: a sensor sample every few seconds, and a client that lives 10 ms
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run(std::env::args().skip(1)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("astroarch-pi-monitor: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> impl Iterator<Item = String> + '_ {
        s.split_whitespace().map(String::from)
    }

    #[test]
    fn parses_set_options() {
        let c = parse_set(args("--temp-warn 65 --volt-crit 4,6 --notifications off")).unwrap();
        assert_eq!(c.temp_warn, Some(65.0));
        assert_eq!(c.volt_crit, Some(4.6));
        assert_eq!(c.notifications, Some(false));
        assert_eq!(c.temp_crit, None);
    }

    #[test]
    fn rejects_bad_set_options() {
        assert!(parse_set(args("")).is_err());
        assert!(parse_set(args("--temp-warn")).is_err());
        assert!(parse_set(args("--temp-warn hot")).is_err());
        assert!(parse_set(args("--notifications maybe")).is_err());
        assert!(parse_set(args("--bogus 1")).is_err());
    }
}

//! QObject exposed to QML: remembers the user's choices for the setup and reads the
//! real state of the system.
//!
//! The choices ("done", "later", "skip", a password kept...) and "hide setup" are kept
//! per user in $XDG_CONFIG_HOME/astroarch/setup.json (~/.config/astroarch/setup.json).
//! What can be read from the system is read every time by the probe helper
//! (scripts/astroarch-control-center-probe.sh): the system is the source of truth, the
//! file only keeps what cannot be read from it.

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use serde_json::{json, Map, Value};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cxx_qt_lib::QString;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        // JSON object: key -> choice ("done", "later", "skip", "kept"...). Read-only in
        // QML: set_choice is the only writer, so every change is saved
        #[qproperty(QString, choices, READ, NOTIFY)]
        // "I'm done, hide setup"
        #[qproperty(bool, hidden, READ, NOTIFY)]
        type SetupState = super::SetupStateRust;

        // Result of one probe: ok = exit code 0, value = its first line of output
        #[qsignal]
        fn probed(self: Pin<&mut SetupState>, name: &QString, ok: bool, value: &QString);

        #[qinvokable]
        fn load(self: Pin<&mut SetupState>);

        // An empty choice forgets the key
        #[qinvokable]
        fn set_choice(self: Pin<&mut SetupState>, key: &QString, choice: &QString);

        #[qinvokable]
        fn hide_setup(self: Pin<&mut SetupState>, hide: bool);

        // `names`: JSON array of probe names. One `probed` signal per name; the results
        // of an older run still in progress are dropped
        #[qinvokable]
        fn run_probes(self: Pin<&mut SetupState>, names: &QString);
    }

    impl cxx_qt::Threading for SetupState {}
}

/// A probe that does not answer within this time counts as "no"
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

const INSTALLED_PROBE: &str = "/usr/lib/astroarch-control-center/astroarch-control-center-probe";

#[derive(Default)]
pub struct SetupStateRust {
    choices: QString,
    hidden: bool,
    generation: Arc<AtomicU64>,
}

fn state_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("astroarch").join("setup.json"))
}

/// The probe helper: $ASTROARCH_CONTROL_CENTER_PROBE, the installed path, else the copy
/// in this crate (development build). Says once on stderr when none is there
fn probe_path() -> PathBuf {
    let candidates = [
        std::env::var_os("ASTROARCH_CONTROL_CENTER_PROBE").map(PathBuf::from),
        Some(PathBuf::from(INSTALLED_PROBE)),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/astroarch-control-center-probe.sh")),
    ];
    if let Some(found) = candidates.iter().flatten().find(|p| p.is_file()) {
        return found.clone();
    }
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        eprintln!("astroarch-control-center: probe helper not found ({INSTALLED_PROBE}): no system state can be read")
    });
    PathBuf::from(INSTALLED_PROBE)
}

/// (choices, hidden). A missing or damaged file gives an empty state
fn read_state(path: &Path) -> (Map<String, Value>, bool) {
    let value: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let choices = value
        .get("choices")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let hidden = value.get("hidden").and_then(Value::as_bool).unwrap_or(false);
    (choices, hidden)
}

/// Written to a temporary file then renamed, so an interrupted save never leaves half a file
fn write_state(path: &Path, choices: &Map<String, Value>, hidden: bool) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&json!({ "choices": choices, "hidden": hidden }))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn parse_choices(choices: &QString) -> Map<String, Value> {
    serde_json::from_str(&choices.to_string()).unwrap_or_default()
}

/// Runs `program arg`; (exit code 0, first line of stdout), within PROBE_TIMEOUT.
/// The probe runs in its own process group, so a timeout also stops the commands of its
/// pipelines; stdout is read by another thread, so neither a full pipe nor a child that
/// keeps it open can block past the timeout
fn run_probe(program: &Path, arg: &str) -> (bool, String) {
    let mut child = match Command::new(program)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return (false, String::new()),
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(mut stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut out = String::new();
            let _ = stdout.read_to_string(&mut out);
            let _ = tx.send(out);
        });
    }
    let kill_group = |child: &mut std::process::Child| {
        // Negative pid: the whole process group (not reaped yet, so not reused)
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .status();
        let _ = child.wait();
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                kill_group(&mut child);
                return (false, String::new());
            }
        }
    };
    // A probe that exited just before the deadline still gets its output read
    let left = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(200));
    match rx.recv_timeout(left) {
        Ok(out) => (
            status.success(),
            out.lines().next().unwrap_or("").trim().to_string(),
        ),
        // Exited, but something it started still holds stdout: stop the group
        Err(_) => {
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{}", child.id())])
                .status();
            (false, String::new())
        }
    }
}

impl qobject::SetupState {
    pub fn load(mut self: Pin<&mut Self>) {
        let (choices, hidden) = state_path().map(|p| read_state(&p)).unwrap_or_default();
        self.as_mut().store(choices, hidden);
    }

    pub fn set_choice(mut self: Pin<&mut Self>, key: &QString, choice: &QString) {
        let mut choices = parse_choices(self.choices());
        let (key, choice) = (key.to_string(), choice.to_string());
        if choice.is_empty() {
            choices.remove(&key);
        } else {
            choices.insert(key, Value::String(choice));
        }
        let hidden = *self.hidden();
        self.as_mut().store(choices, hidden);
        self.save();
    }

    pub fn hide_setup(mut self: Pin<&mut Self>, hide: bool) {
        let choices = parse_choices(self.choices());
        self.as_mut().store(choices, hide);
        self.save();
    }

    pub fn run_probes(self: Pin<&mut Self>, names: &QString) {
        let names: Vec<String> = serde_json::from_str(&names.to_string()).unwrap_or_default();
        let generation = self.rust().generation.clone();
        let run = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let qt_thread = self.qt_thread();
        let program = probe_path();

        // All at once: a slow probe (a server, a big folder) does not delay the others
        for name in names {
            let (generation, qt_thread, program) =
                (generation.clone(), qt_thread.clone(), program.clone());
            std::thread::spawn(move || {
                let (ok, value) = run_probe(&program, &name);
                qt_thread
                    .queue(move |mut qobject| {
                        // A newer run started: it reports instead
                        if generation.load(Ordering::SeqCst) == run {
                            qobject
                                .as_mut()
                                .probed(&QString::from(&name), ok, &QString::from(&value));
                        }
                    })
                    .ok();
            });
        }
    }

    /// Updates both properties and tells QML
    fn store(mut self: Pin<&mut Self>, choices: Map<String, Value>, hidden: bool) {
        let text = QString::from(&Value::Object(choices).to_string());
        if *self.choices() != text {
            self.as_mut().rust_mut().choices = text;
            self.as_mut().choices_changed();
        }
        if *self.hidden() != hidden {
            self.as_mut().rust_mut().hidden = hidden;
            self.as_mut().hidden_changed();
        }
    }

    fn save(&self) {
        let Some(path) = state_path() else { return };
        if let Err(err) = write_state(&path, &parse_choices(self.choices()), *self.hidden()) {
            eprintln!(
                "astroarch-control-center: cannot save {}: {err}",
                path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("astroarch-setup-test-{}-{name}", std::process::id()))
            .join("setup.json")
    }

    fn script(name: &str, body: &str) -> PathBuf {
        let path = temp_file(name).with_file_name("probe.sh");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(&path, perm).unwrap();
        path
    }

    #[test]
    fn state_round_trip() {
        let path = temp_file("round-trip");
        let mut choices = Map::new();
        choices.insert("network".into(), Value::String("done".into()));
        choices.insert("password-user".into(), Value::String("kept".into()));
        write_state(&path, &choices, true).unwrap();

        let (read, hidden) = read_state(&path);
        assert_eq!(read, choices);
        assert!(hidden);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_or_damaged_file_is_empty() {
        let path = temp_file("damaged");
        assert_eq!(read_state(&path), (Map::new(), false));

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(read_state(&path), (Map::new(), false));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn probes_report_exit_code_and_first_line() {
        let ok = script("ok", "echo \"value for $1\"; echo second line");
        assert_eq!(run_probe(&ok, "x"), (true, "value for x".to_string()));
        let fail = script("fail", "exit 3");
        assert_eq!(run_probe(&fail, "x"), (false, String::new()));
        let _ = std::fs::remove_dir_all(ok.parent().unwrap());
        let _ = std::fs::remove_dir_all(fail.parent().unwrap());
    }

    #[test]
    fn a_child_holding_stdout_does_not_block() {
        let held = script("held", "sleep 30 & echo early");
        let start = Instant::now();
        let (ok, _) = run_probe(&held, "x");
        assert!(!ok);
        assert!(start.elapsed() < PROBE_TIMEOUT + Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(held.parent().unwrap());
    }

    #[test]
    fn the_probe_script_is_found_in_a_development_build() {
        assert!(probe_path().is_file());
    }
}

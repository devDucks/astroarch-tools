//! QObject exposed to QML: starts external commands / applications.
//!
//! `launch(command, in_terminal)`:
//!   - in_terminal = true  -> opens a separate terminal window running `command`
//!                            (the window stays open at the end until Enter is pressed)
//!   - in_terminal = false -> starts `command` as a detached GUI application
//!
//! In both cases the words after the first are its arguments (e.g. "kcmshell6 kcm_users");
//! quoting is not supported. Returns "started", "running" (the same command is still
//! open: it is not started twice), "missing" or "failed", so QML can tell the user.
//! `finished(command)` is emitted when a command started here exits.

use std::collections::HashSet;
use std::env;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use cxx_qt::Threading;
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
        type Launcher = super::LauncherRust;

        #[qinvokable]
        fn launch(self: &Launcher, command: &QString, in_terminal: bool) -> QString;

        // Is the program of a GUI command installed (or next to this executable)?
        #[qinvokable]
        fn available(self: &Launcher, command: &QString) -> bool;

        #[qsignal]
        fn finished(self: Pin<&mut Launcher>, command: &QString);
    }

    impl cxx_qt::Threading for Launcher {}
}

#[derive(Default)]
pub struct LauncherRust {
    /// Commands started here that have not exited yet
    running: Arc<Mutex<HashSet<String>>>,
}

/// Terminal emulators tried in order, with the arguments that precede the command to run.
const TERMINALS: &[(&str, &[&str])] = &[
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("lxterminal", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("foot", &[]),
    ("xterm", &["-e"]),
];

/// Wrapper run in the terminal: runs the command, then waits for Enter so the output stays
/// readable. A program is run by sh; a shell function such as `update-astroarch` needs an
/// *interactive* zsh (zsh -ic) that loads it from ~/.zshrc. (`status` is read-only in zsh,
/// hence `rc`.)
const WRAPPER: &str = r#""$@"; rc=$?; printf '\n[%s finished with code %s] Press Enter to close...' "$1" "$rc"; read -r dummy"#;

fn find_in_path(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let p = PathBuf::from(program);
        return p.is_file().then_some(p);
    }
    env::var_os("PATH")
        .and_then(|paths| {
            env::split_paths(&paths)
                .map(|dir| dir.join(program))
                .find(|candidate| candidate.is_file())
        })
        // Not installed (development build): the other tools are next to this executable
        .or_else(|| {
            let exe = env::current_exe().ok()?;
            let candidate = exe.parent()?.join(program);
            candidate.is_file().then_some(candidate)
        })
}

fn spawn_detached(program: &Path, args: &[String]) -> Option<Child> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0) // detach from the launcher's process group
        .spawn()
        .map_err(|err| {
            eprintln!(
                "astroarch-control-center: cannot start {}: {err}",
                program.display()
            )
        })
        .ok()
}

fn spawn_in_terminal(words: &[String]) -> Result<Child, &'static str> {
    let mut words = words.to_vec();
    let (shell, shell_args): (PathBuf, &[&str]) = match find_in_path(&words[0]) {
        // A program (also one found next to this executable): run by its full path with sh
        Some(path) => {
            words[0] = path.to_string_lossy().into_owned();
            (PathBuf::from("/bin/sh"), &["-c"])
        }
        // A shell function: only an interactive zsh with a ~/.zshrc knows it. Without
        // ~/.zshrc, zsh -ic would open its new-user menu instead
        None => {
            let zshrc = env::var_os("HOME").map(|h| PathBuf::from(h).join(".zshrc"));
            match find_in_path("zsh") {
                Some(zsh) if zshrc.is_some_and(|z| z.is_file()) => (zsh, &["-ic"]),
                _ => return Err("missing"),
            }
        }
    };

    for (terminal, prefix) in TERMINALS {
        if let Some(terminal_path) = find_in_path(terminal) {
            let mut args: Vec<String> = prefix.iter().map(|s| s.to_string()).collect();
            args.push(shell.to_string_lossy().into_owned());
            args.extend(shell_args.iter().map(|s| s.to_string()));
            args.extend([
                WRAPPER.to_string(),
                "sh".to_string(), // $0 of the wrapper script
            ]);
            args.extend(words); // $1... = the command and its arguments, run through "$@"
            return spawn_detached(&terminal_path, &args).ok_or("failed");
        }
    }

    eprintln!("astroarch-control-center: no supported terminal emulator found");
    Err("missing")
}

impl qobject::Launcher {
    pub fn launch(&self, command: &QString, in_terminal: bool) -> QString {
        let command = command.to_string().trim().to_string();
        let words: Vec<String> = command.split_whitespace().map(String::from).collect();
        if words.is_empty() {
            return QString::from("failed");
        }

        // The same tool opened with other options (the network settings on the hotspot,
        // the language module on its keyboard page) counts as the same window
        let key = command.split(" --").next().unwrap_or(&command).to_string();
        let running = self.running.clone();
        if running.lock().map(|r| r.contains(&key)).unwrap_or(false) {
            return QString::from("running");
        }

        let child = if in_terminal {
            spawn_in_terminal(&words)
        } else {
            match find_in_path(&words[0]) {
                Some(path) => spawn_detached(&path, &words[1..]).ok_or("failed"),
                None => {
                    eprintln!("astroarch-control-center: command not found: {}", words[0]);
                    Err("missing")
                }
            }
        };

        match child {
            Ok(mut child) => {
                if let Ok(mut r) = running.lock() {
                    r.insert(key.clone());
                }
                let qt_thread = self.qt_thread();
                // Reap the child when it exits (no zombie), then tell QML
                std::thread::spawn(move || {
                    let _ = child.wait();
                    if let Ok(mut r) = running.lock() {
                        r.remove(&key);
                    }
                    qt_thread
                        .queue(move |mut qobject| {
                            qobject.as_mut().finished(&QString::from(&command));
                        })
                        .ok();
                });
                QString::from("started")
            }
            Err(reason) => QString::from(reason),
        }
    }

    pub fn available(&self, command: &QString) -> bool {
        command
            .to_string()
            .split_whitespace()
            .next()
            .and_then(find_in_path)
            .is_some()
    }
}

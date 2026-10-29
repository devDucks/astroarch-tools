//! QObject exposed to QML: starts external commands / applications.
//!
//! `launch(command, in_terminal)`:
//!   - in_terminal = true  -> opens a separate terminal window running `command`
//!                            (the window stays open at the end until Enter is pressed)
//!   - in_terminal = false -> starts `command` as a detached GUI application
//!
//! Returns false if the command cannot be found or started, so QML can notify the user.

use std::env;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cxx_qt_lib::QString;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type Launcher = super::LauncherRust;

        #[qinvokable]
        fn launch(self: &Launcher, command: &QString, in_terminal: bool) -> bool;
    }
}

#[derive(Default)]
pub struct LauncherRust {}

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

/// Wrapper run by an *interactive* shell (zsh -ic), so that zsh plugins/functions such as
/// `update-astroarch` are loaded from ~/.zshrc. It runs the command, then waits for Enter
/// so the output stays readable. (`status` is read-only in zsh, hence `rc`.)
const WRAPPER: &str = r#""$@"; rc=$?; printf '\n[%s finished with code %s] Press Enter to close...' "$1" "$rc"; read -r dummy"#;

fn find_in_path(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let p = PathBuf::from(program);
        return p.is_file().then_some(p);
    }
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
    })
}

fn spawn_detached(program: &Path, args: &[String]) -> bool {
    let result = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0) // detach from the launcher's process group
        .spawn();

    match result {
        Ok(mut child) => {
            // Reap the child when it exits so no zombie stays around.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(err) => {
            eprintln!(
                "astroarch-control-center: cannot start {}: {err}",
                program.display()
            );
            false
        }
    }
}

fn launch_in_terminal(command: &str) -> bool {
    // The command may be a zsh plugin/function (not a file in PATH), so it is not checked
    // here: if it does not exist, the error is simply shown in the terminal window.
    let (shell_path, shell_name) = match find_in_path("zsh") {
        Some(path) => (path, "zsh"),
        None => match find_in_path("bash") {
            Some(path) => (path, "bash"),
            None => {
                eprintln!("astroarch-control-center: neither zsh nor bash found");
                return false;
            }
        },
    };

    for (terminal, prefix) in TERMINALS {
        if let Some(terminal_path) = find_in_path(terminal) {
            let mut args: Vec<String> = prefix.iter().map(|s| s.to_string()).collect();
            args.extend([
                shell_path.to_string_lossy().into_owned(),
                "-ic".to_string(), // interactive: loads ~/.zshrc (plugins, functions)
                WRAPPER.to_string(),
                shell_name.to_string(), // $0 of the wrapper script
                command.to_string(),    // $1 = command, run through "$@"
            ]);
            return spawn_detached(&terminal_path, &args);
        }
    }

    eprintln!("astroarch-control-center: no supported terminal emulator found");
    false
}

impl qobject::Launcher {
    pub fn launch(&self, command: &QString, in_terminal: bool) -> bool {
        let command = command.to_string();
        let command = command.trim();
        if command.is_empty() {
            return false;
        }

        if in_terminal {
            launch_in_terminal(command)
        } else {
            match find_in_path(command) {
                Some(path) => spawn_detached(&path, &[]),
                None => {
                    eprintln!("astroarch-control-center: command not found: {command}");
                    false
                }
            }
        }
    }
}

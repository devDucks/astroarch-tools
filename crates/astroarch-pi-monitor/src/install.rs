//! `astroarch-pi-monitor install`: one command puts everything in place. The widget and the
//! service unit are embedded in the binary, so the binary alone is enough:
//!
//!   user (default)  ~/.local/bin, ~/.local/share/plasma/plasmoids, ~/.config/systemd/user;
//!                   the service is enabled and started now
//!   --system (root) /usr/local/bin, /usr/share/plasma/plasmoids, /etc/systemd/user;
//!                   the service is enabled for every user (from their next login)
//!
//! The widget reads the service with Plasma's own D-Bus module (see Backend.qml of the
//! widget): nothing of ours is loaded into the Plasma shell.

use crate::config::{config_path, Config};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN_NAME: &str = "astroarch-pi-monitor";
const PLASMOID_ID: &str = "org.astroarch.pimonitor";
const SERVICE: &str = "astroarch-pi-monitor.service";
const PLACEHOLDER: &str = "@BIN@";

/// Left by the first version (a QML plugin and the script that put it in the Plasma
/// shell's import path), relative to the home: removed by `install` and `uninstall`
const OLD_USER_FILES: [&str; 2] = [
    ".local/lib/qt6/qml/org/astroarch/pimonitor",
    ".config/plasma-workspace/env/astroarch-pi-monitor.sh",
];

/// The widget's files, relative to its directory
const PLASMOID_FILES: [(&str, &str); 6] = [
    ("metadata.json", include_str!("../plasmoid/org.astroarch.pimonitor/metadata.json")),
    ("contents/config/main.xml", include_str!("../plasmoid/org.astroarch.pimonitor/contents/config/main.xml")),
    ("contents/config/config.qml", include_str!("../plasmoid/org.astroarch.pimonitor/contents/config/config.qml")),
    ("contents/ui/main.qml", include_str!("../plasmoid/org.astroarch.pimonitor/contents/ui/main.qml")),
    ("contents/ui/configGeneral.qml", include_str!("../plasmoid/org.astroarch.pimonitor/contents/ui/configGeneral.qml")),
    // The only one that runs the command: it needs the binary's path
    ("contents/ui/Backend.qml", include_str!("../plasmoid/org.astroarch.pimonitor/contents/ui/Backend.qml")),
];
const UNIT: &str = include_str!("../systemd/astroarch-pi-monitor.service");

/// The alert icons (widget and notifications), in the hicolor theme
const ICONS: [(&str, &str); 2] = [
    ("org.astroarch.pimonitor-warning.svg", include_str!("../icons/org.astroarch.pimonitor-warning.svg")),
    ("org.astroarch.pimonitor-critical.svg", include_str!("../icons/org.astroarch.pimonitor-critical.svg")),
];

struct Paths {
    bin: PathBuf,
    plasmoid: PathBuf,
    unit: PathBuf,
    /// hicolor/scalable/status, where the alert icons go
    icons: PathBuf,
    /// A user install: the home, where the first version left files
    home: Option<PathBuf>,
}

fn paths(system: bool) -> Result<Paths, String> {
    if system {
        return Ok(Paths {
            bin: PathBuf::from("/usr/local/bin").join(BIN_NAME),
            plasmoid: PathBuf::from("/usr/share/plasma/plasmoids").join(PLASMOID_ID),
            unit: PathBuf::from("/etc/systemd/user").join(SERVICE),
            icons: PathBuf::from("/usr/share/icons/hicolor/scalable/status"),
            home: None,
        });
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is not set")?;
    let abs = |var: &str, fallback: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(fallback))
    };
    Ok(Paths {
        bin: home.join(".local/bin").join(BIN_NAME),
        plasmoid: abs("XDG_DATA_HOME", ".local/share").join("plasma/plasmoids").join(PLASMOID_ID),
        unit: abs("XDG_CONFIG_HOME", ".config").join("systemd/user").join(SERVICE),
        icons: abs("XDG_DATA_HOME", ".local/share").join("icons/hicolor/scalable/status"),
        home: Some(home),
    })
}

/// A string for the inside of a QML string literal
fn qml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn render_qml(template: &str, bin: &Path) -> String {
    template.replace(PLACEHOLDER, &qml_escape(&bin.to_string_lossy()))
}

pub fn render_unit(bin: &Path) -> String {
    UNIT.replace(PLACEHOLDER, &bin.to_string_lossy())
}

fn io<T>(r: std::io::Result<T>, what: &Path) -> Result<T, String> {
    r.map_err(|e| {
        let hint = if e.kind() == std::io::ErrorKind::PermissionDenied { " (run it as root: sudo ... --system)" } else { "" };
        format!("{}: {e}{hint}", what.display())
    })
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        io(fs::create_dir_all(dir), dir)?;
    }
    io(fs::write(path, content), path)
}

/// Copies the running binary to `dest`, through a temporary file: a running daemon keeps
/// working with the old one until it is restarted
fn install_binary(exe: &Path, dest: &Path) -> Result<(), String> {
    if let (Ok(a), Ok(b)) = (exe.canonicalize(), dest.canonicalize()) {
        if a == b {
            return Ok(());
        }
    }
    if let Some(dir) = dest.parent() {
        io(fs::create_dir_all(dir), dir)?;
    }
    let tmp = dest.with_extension("new");
    io(fs::copy(exe, &tmp), dest)?;
    io(fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)), &tmp)?;
    io(fs::rename(&tmp, dest), dest)
}

fn write_plasmoid(dir: &Path, bin: &Path) -> Result<(), String> {
    // A clean upgrade: no file of an older version stays
    if dir.exists() {
        io(fs::remove_dir_all(dir), dir)?;
    }
    for (rel, content) in PLASMOID_FILES {
        write_file(&dir.join(rel), &render_qml(content, bin))?;
    }
    Ok(())
}

/// Runs systemctl; a failure is only said (no user session when run from ssh, for example)
fn systemctl(args: &[&str]) {
    match Command::new("systemctl").args(args).status() {
        Ok(s) if s.success() => {}
        _ => eprintln!("warning: `systemctl {}` failed; run it yourself in your session", args.join(" ")),
    }
}

/// Removes what the first version left in the home (QML plugin, session script)
fn remove_old_user_files(p: &Paths) -> Result<(), String> {
    let Some(home) = &p.home else { return Ok(()) };
    for rel in OLD_USER_FILES {
        let path = home.join(rel);
        if path.is_dir() {
            io(fs::remove_dir_all(&path), &path)?;
        } else if path.exists() {
            io(fs::remove_file(&path), &path)?;
        } else {
            continue;
        }
        println!("removed   {} (from an older version)", path.display());
    }
    Ok(())
}

pub fn install(system: bool) -> Result<(), String> {
    let p = paths(system)?;
    if p.bin.to_string_lossy().contains(char::is_whitespace) {
        return Err(format!("{}: the path of the binary must not contain spaces", p.bin.display()));
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the running binary: {e}"))?;

    install_binary(&exe, &p.bin)?;
    println!("binary    {}", p.bin.display());
    write_plasmoid(&p.plasmoid, &p.bin)?;
    println!("widget    {}", p.plasmoid.display());
    for (name, content) in ICONS {
        write_file(&p.icons.join(name), content)?;
    }
    println!("icons     {}", p.icons.display());
    write_file(&p.unit, &render_unit(&p.bin))?;
    println!("service   {}", p.unit.display());
    remove_old_user_files(&p)?;

    if system {
        systemctl(&["--global", "enable", SERVICE]);
        println!("\nThe service starts at the next login of each user. To start it now, as the user:");
        println!("  systemctl --user enable --now {SERVICE}");
    } else {
        if !config_path().exists() {
            Config::default().save()?;
            println!("settings  {}", config_path().display());
        }
        systemctl(&["--user", "daemon-reload"]);
        // enable + restart: also loads a new version of the binary when upgrading
        systemctl(&["--user", "enable", SERVICE]);
        systemctl(&["--user", "restart", SERVICE]);
    }

    println!("\nTo add the widget: right-click the desktop or a panel, \"Add Widgets…\", then \"AstroArch Pi Monitor\".");
    println!("If it is not in the list yet, restart the Plasma shell (it keeps its windows):");
    println!("  systemctl --user restart plasma-plasmashell   # or: kquitapp6 plasmashell; kstart plasmashell");
    Ok(())
}

pub fn uninstall(system: bool) -> Result<(), String> {
    let p = paths(system)?;
    if system {
        systemctl(&["--global", "disable", SERVICE]);
    } else {
        systemctl(&["--user", "disable", "--now", SERVICE]);
    }
    let icons: Vec<PathBuf> = ICONS.iter().map(|(name, _)| p.icons.join(name)).collect();
    for path in [&p.unit, &p.bin].into_iter().chain(&icons) {
        if path.exists() {
            io(fs::remove_file(path), path)?;
            println!("removed   {}", path.display());
        }
    }
    if p.plasmoid.exists() {
        io(fs::remove_dir_all(&p.plasmoid), &p.plasmoid)?;
        println!("removed   {}", p.plasmoid.display());
    }
    remove_old_user_files(&p)?;
    if !system {
        systemctl(&["--user", "daemon-reload"]);
        println!("kept      {} (your settings)", config_path().display());
    }
    println!("\nRemove the widget from your panels, then restart the Plasma shell if it is still shown.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_gets_the_binary_path() {
        let bin = Path::new("/home/astronaut/.local/bin/astroarch-pi-monitor");
        for (name, content) in PLASMOID_FILES {
            let out = render_qml(content, bin);
            assert!(!out.contains(PLACEHOLDER), "{name} still has the placeholder");
            // Only the command backend needs the binary's path
            if name.ends_with("Backend.qml") {
                assert!(content.contains(PLACEHOLDER), "{name} should use the binary path");
                assert!(out.contains("/home/astronaut/.local/bin/astroarch-pi-monitor"));
            } else {
                assert!(!content.contains(PLACEHOLDER), "{name} should not need the binary path");
            }
        }
        let unit = render_unit(bin);
        assert!(unit.contains("ExecStart=/home/astronaut/.local/bin/astroarch-pi-monitor daemon"));
    }

    #[test]
    fn the_widget_uses_plasmas_dbus_module() {
        let backend = PLASMOID_FILES.iter().find(|(n, _)| n.ends_with("Backend.qml")).unwrap().1;
        assert!(backend.contains("import org.kde.plasma.workspace.dbus"));
        assert!(!backend.contains("import org.astroarch.pimonitor"));
    }

    #[test]
    fn the_alert_icons_are_the_ones_the_widget_and_the_daemon_use() {
        let main = PLASMOID_FILES.iter().find(|(n, _)| n.ends_with("main.qml")).unwrap().1;
        let daemon = include_str!("daemon.rs");
        for (file, svg) in ICONS {
            let name = file.trim_end_matches(".svg");
            assert!(main.contains(name), "main.qml does not use {name}");
            assert!(daemon.contains(name), "daemon.rs does not use {name}");
            assert!(svg.contains("<svg"));
        }
    }

    #[test]
    fn the_path_is_escaped_for_qml() {
        assert_eq!(qml_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn the_widget_metadata_matches_its_directory() {
        let v: serde_json::Value = serde_json::from_str(PLASMOID_FILES[0].1).unwrap();
        assert_eq!(v["KPlugin"]["Id"], PLASMOID_ID);
        assert_eq!(v["KPackageStructure"], "Plasma/Applet");
    }

    #[test]
    fn installs_and_uninstalls_the_widget_files() {
        let dir = std::env::temp_dir().join(format!("pi-monitor-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let plasmoid = dir.join(PLASMOID_ID);
        write_plasmoid(&plasmoid, Path::new("/usr/local/bin/astroarch-pi-monitor")).unwrap();
        for (rel, _) in PLASMOID_FILES {
            assert!(plasmoid.join(rel).is_file(), "{rel} missing");
        }
        // An upgrade replaces the directory: a stale file does not stay
        fs::write(plasmoid.join("contents/ui/old.qml"), "x").unwrap();
        write_plasmoid(&plasmoid, Path::new("/usr/local/bin/astroarch-pi-monitor")).unwrap();
        assert!(!plasmoid.join("contents/ui/old.qml").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}

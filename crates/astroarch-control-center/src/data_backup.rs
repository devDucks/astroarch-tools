//! QObject exposed to QML: the "Data and settings" backup in the Data backup page.
//!
//! All the backup logic is the backup tool's own (crates/astroarch-backup): the catalog and
//! the plan (data_profile.rs), the drives (media.rs, storage.rs) and the root helper that
//! creates the destination folder. This file only reads them for the page, saves the
//! user's choices, and switches the timer (astroarch-data-backup.timer, a user unit).
//! The backup itself runs as `astroarch-backup --data-backup`, by the timer or by
//! "Back up now", so it goes on when the window is closed.

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::data_profile::{self as profile, Config, Destination, Homes};
use crate::{drives, media, storage};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        // A setup step (folder creation, mounting) is in progress
        #[qproperty(bool, busy)]
        type DataBackup = super::DataBackupRust;

        // Size in bytes of one row of the list (key: item id, "item/child" or a path)
        #[qsignal]
        #[cxx_name = "sizeReady"]
        fn size_ready(self: Pin<&mut DataBackup>, key: &QString, bytes: f64);

        // A drive was mounted (empty mountpoint and an error when it failed)
        #[qsignal]
        fn mounted(self: Pin<&mut DataBackup>, device: &QString, mountpoint: &QString, error: &QString);

        // The choices were saved; error: "" or a code / message
        #[qsignal]
        fn applied(self: Pin<&mut DataBackup>, ok: bool, error: &QString);

        // A network share was connected or removed; path: its mount point
        #[qsignal]
        #[cxx_name = "shareChanged"]
        fn share_changed(self: Pin<&mut DataBackup>, ok: bool, error: &QString, path: &QString);

        // `share`: JSON {kind: "smb"|"nfs", server, share, user, password}. Checked here,
        // then added by the root helper (nas-add) as a share mounted when used
        #[qinvokable]
        #[cxx_name = "connectShare"]
        fn connect_share(self: Pin<&mut DataBackup>, share: &QString);

        // Forget a share added by connectShare (its mount point)
        #[qinvokable]
        #[cxx_name = "removeShare"]
        fn remove_share(self: Pin<&mut DataBackup>, path: &QString);

        // JSON: [{id, title, items: [{id, name, paths, exists, essential, warn, hint,
        // neededBy, subfolders, children: [{key, name, paths, exists, note}]}]}]
        #[qinvokable]
        fn rows(self: &DataBackup) -> QString;

        // Sizes of every row, sent one by one (sizeReady)
        #[qinvokable]
        #[cxx_name = "requestSizes"]
        fn request_sizes(self: Pin<&mut DataBackup>);

        // JSON: {configured, excluded, added, keep, destination: {kind, path, label}}
        #[qinvokable]
        fn config(self: &DataBackup) -> QString;

        // JSON: [{kind, path, label, detail, free, state, device?, hasBackup}] for `bytes`
        // of data. state: ready, too_small, not_mounted, fat32, nohistory, readonly
        #[qinvokable]
        fn destinations(self: &DataBackup, bytes: f64) -> QString;

        #[qinvokable]
        fn mount(self: Pin<&mut DataBackup>, device: &QString);

        // `choices`: JSON {excluded, added, keep, destination, schedule}
        #[qinvokable]
        fn apply(self: Pin<&mut DataBackup>, choices: &QString);

        // Timer on or off; returns "" or an error message
        #[qinvokable]
        #[cxx_name = "setSchedule"]
        fn set_schedule(self: &DataBackup, on: bool) -> QString;

        // JSON: {installed, enabled, next (seconds since 1970, 0 = none)}
        #[qinvokable]
        fn schedule(self: &DataBackup) -> QString;

        // JSON: the last run (see data_profile.rs) + active (a backup runs now)
        #[qinvokable]
        fn status(self: &DataBackup) -> QString;

        // "started", "running" or "missing"
        #[qinvokable]
        #[cxx_name = "runNow"]
        fn run_now(self: &DataBackup) -> QString;

        // A folder the user adds: "" when fine, else missing | too_wide | destination
        #[qinvokable]
        #[cxx_name = "checkFolder"]
        fn check_folder(self: &DataBackup, path: &QString) -> QString;

        // JSON {dest, keep}: the system backup's last place (the backup tool's config)
        #[qinvokable]
        #[cxx_name = "systemConfig"]
        fn system_config(self: &DataBackup) -> QString;

        // The system backup's folders left out (one absolute path per line), saved in the
        // backup tool's config; returns "" or an error
        #[qinvokable]
        #[cxx_name = "saveSystemExcluded"]
        fn save_system_excluded(self: &DataBackup, paths: &QString) -> QString;

        // JSON: the folders of the home, as suggestions to add
        #[qinvokable]
        #[cxx_name = "homeFolders"]
        fn home_folders(self: &DataBackup) -> QString;

        // The drives of a JSON list of BackupManager.list_media() that can be offered:
        // not another system's disk (drives.rs, as for the data backup)
        #[qinvokable]
        #[cxx_name = "offeredDrives"]
        fn offered_drives(self: &DataBackup, media: &QString) -> QString;
    }

    impl cxx_qt::Threading for DataBackup {}
}

#[derive(Default)]
pub struct DataBackupRust {
    busy: bool,
}

fn q(text: &str) -> QString {
    QString::from(text)
}

/// Can this user write in `path`? (test -w: also ACLs and read-only mounts)
fn writable(path: &Path) -> bool {
    Command::new("test")
        .arg("-w")
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(program))
            .find(|p| p.is_file())
    })
}

/// The backup tool: installed, or next to this executable (development build)
fn backup_tool() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("astroarch-backup")))
        .filter(|p| p.is_file())
        .or_else(|| which("astroarch-backup"))
}

fn systemctl_user(args: &[&str]) -> (bool, String) {
    match Command::new("systemctl").arg("--user").args(args).output() {
        Ok(out) => (
            out.status.success(),
            format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
                .trim()
                .to_string(),
        ),
        Err(e) => (false, e.to_string()),
    }
}

/// Is a backup running now? It holds the lock file while it runs
fn backup_active() -> bool {
    match std::fs::File::open(profile::lock_path()) {
        Ok(f) => f.try_lock_shared().is_err(),
        Err(_) => false,
    }
}

/// Shares added from the page (helper nas-add) are mounted here when used
const NAS_BASE: &str = "/mnt/astroarch-nas";

/// The rules of the helper's nas-add, checked before anything reaches pkexec. Err: a code
/// the page translates (server, share, export, user, password)
pub fn check_share(kind: &str, server: &str, share: &str, user: &str, password: &str) -> Result<(), &'static str> {
    let host_ok = !server.is_empty()
        && server.len() <= 253
        && server.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && server.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    let ipv6_ok = server.len() >= 2 && server.len() <= 41 && server.chars().all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '[' | ']'));
    if !(host_ok || ipv6_ok) {
        return Err("server");
    }
    match kind {
        "smb" => {
            if share.is_empty() || share.len() > 80 || share.contains("..")
                || !share.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ' ' | '$' | '-'))
            {
                return Err("share");
            }
            if user.len() > 64 || !user.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '-')) {
                return Err("user");
            }
            if password.contains(['\n', '\r']) {
                return Err("password");
            }
        }
        "nfs" => {
            if !share.starts_with('/') || share.len() > 201 || share.contains("..")
                || !share.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
            {
                return Err("export");
            }
        }
        _ => return Err("share"),
    }
    Ok(())
}

/// Runs the root helper through pkexec, `input` on its stdin. Ok: the RESULT value
fn run_helper(args: &[&str], input: &str) -> Result<String, String> {
    use std::io::Write;
    let helper = storage::helper_path().ok_or("Helper script not found (astroarch-backup-helper).")?;
    let mut child = Command::new("pkexec")
        .arg(helper)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        return Ok(stdout.lines().find_map(|l| l.strip_prefix("RESULT:ok:")).unwrap_or("").to_string());
    }
    Err(match out.status.code() {
        Some(126) | Some(127) => "auth".into(),
        _ => stdout
            .lines()
            .find_map(|l| l.strip_prefix("ERROR:").map(|r| r.splitn(2, ':').nth(1).unwrap_or(r).to_string()))
            .unwrap_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_string()),
    })
}

/// Shares added from the page that are not mounted now (automount, mounted when used):
/// (mount point, source from /etc/fstab)
fn idle_shares(mounted: &[(String, String, String)]) -> Vec<(String, String)> {
    let Ok(out) = Command::new("findmnt").args(["-rn", "--fstab", "-o", "TARGET,SOURCE"]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut parts = l.split(' ').map(|s| s.replace("\\x20", " ").replace("\\040", " "));
            Some((parts.next()?, parts.next()?))
        })
        .filter(|(target, _)| target.starts_with(&format!("{NAS_BASE}/")) && !mounted.iter().any(|(t, _, _)| t == target))
        .collect()
}

/// state of a place `path` for `bytes` of data, 1 GiB kept free. A place that already holds
/// the backup only gets what changed: the 1 GiB is enough
fn space_state(free: u64, bytes: u64, path: &Path) -> &'static str {
    // This system's own backup folder there (<path>/<id>), not just any AstroArch backup:
    // only then is the next run small
    let id = profile::load_config().map(|c| c.id).unwrap_or_default();
    let needed = if !id.is_empty() && path.join(&id).is_dir() { 0 } else { bytes };
    if free >= needed.saturating_add(storage::GIB) {
        "ready"
    } else {
        "too_small"
    }
}

/// Mounted network shares: (mount point, source, type)
fn network_mounts() -> Vec<(String, String, String)> {
    let out = Command::new("findmnt")
        .args(["-rn", "-o", "TARGET,SOURCE,FSTYPE", "-t", "cifs,smb3,nfs,nfs4,fuse.sshfs"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            // findmnt -r writes a space in a name as \x20
            let mut parts = l.split(' ').map(|s| s.replace("\\x20", " "));
            Some((parts.next()?, parts.next()?, parts.next()?))
        })
        .collect()
}

impl qobject::DataBackup {
    pub fn rows(&self) -> QString {
        q(&profile::rows(&profile::catalog(), &Homes::current()).to_string())
    }

    pub fn request_sizes(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            for (key, paths) in profile::size_keys(&profile::catalog(), &Homes::current()) {
                let bytes = profile::size_of(&paths) as f64;
                qt_thread
                    .queue(move |mut o| o.as_mut().size_ready(&q(&key), bytes))
                    .ok();
            }
        });
    }

    pub fn config(&self) -> QString {
        let value = match profile::load_config() {
            Some(c) => json!({ "configured": !c.destination.path.is_empty(), "excluded": c.excluded,
                               "added": c.added, "keep": c.keep, "destination": c.destination }),
            None => json!({ "configured": false, "excluded": profile::default_excluded(&profile::catalog()),
                            "added": [], "keep": profile::DEFAULT_KEEP, "destination": {} }),
        };
        q(&value.to_string())
    }

    pub fn destinations(&self, bytes: f64) -> QString {
        let bytes = bytes.max(0.0) as u64;
        let homes = Homes::current();
        let mut list = Vec::new();

        // Main disk: ready at once, but lost with the SD card
        let internal = Path::new(profile::INTERNAL_BASE).join(&homes.user);
        let free = storage::available_space(&storage::existing_ancestor(&internal)).unwrap_or(0);
        list.push(json!({ "kind": "internal", "path": internal, "label": "", "detail": profile::INTERNAL_BASE,
                          "free": free, "state": space_state(free, bytes, &internal), "hasBackup": internal.is_dir() }));

        // Drives that are not the system disk nor another system's (the backup tool's list)
        for m in drives::list(0) {
            let fstype = m["fstype"].as_str().unwrap_or("");
            let mountpoint = m["mountpoint"].as_str().unwrap_or("").to_string();
            let path = if mountpoint.is_empty() { String::new() } else { format!("{mountpoint}/{}", profile::MEDIA_SUBDIR) };
            let free = if mountpoint.is_empty() { 0 } else { storage::available_space(Path::new(&mountpoint)).unwrap_or(0) };
            let state = match (m["state"].as_str().unwrap_or(""), fstype) {
                ("fat32", _) => "fat32",
                // Neither permissions nor hard links: no history
                (_, "exfat" | "ntfs" | "ntfs3" | "ntfs-3g") => "nohistory",
                ("not_mounted", _) => "not_mounted",
                ("readonly", _) => "readonly",
                _ => space_state(free, bytes, Path::new(&path)),
            };
            list.push(json!({ "kind": "usb", "path": path, "label": m["label"], "device": m["device"],
                              "detail": format!("{} · {}", m["device"].as_str().unwrap_or(""), fstype),
                              "free": free, "state": state, "hasBackup": !path.is_empty() && Path::new(&path).is_dir() }));
        }

        // Network shares already mounted (a NAS), and those added here, mounted when used
        let mounted = network_mounts();
        for (target, source) in idle_shares(&mounted) {
            list.push(json!({ "kind": "nas", "path": format!("{target}/{}", profile::MEDIA_SUBDIR), "label": source,
                              "detail": target, "free": 0, "state": "automount", "hasBackup": false,
                              "mountPoint": target, "removable": true }));
        }
        for (target, source, fstype) in mounted {
            let path = format!("{target}/{}", profile::MEDIA_SUBDIR);
            let free = storage::available_space(Path::new(&target)).unwrap_or(0);
            let state = if writable(Path::new(&path)) || (!Path::new(&path).exists() && writable(Path::new(&target))) {
                space_state(free, bytes, Path::new(&path))
            } else {
                "readonly"
            };
            list.push(json!({ "kind": "nas", "path": path, "label": source, "detail": format!("{target} · {fstype}"),
                              "free": free, "state": state, "hasBackup": Path::new(&path).is_dir(),
                              "mountPoint": target, "removable": target.starts_with(&format!("{NAS_BASE}/")) }));
        }

        // Syncthing: the backup on the main disk, sent to the computer
        if which("syncthing").is_some() {
            list.push(json!({ "kind": "syncthing", "path": internal, "label": "", "detail": profile::INTERNAL_BASE,
                              "free": free_of(&internal), "state": space_state(free_of(&internal), bytes, &internal),
                              "hasBackup": internal.is_dir() }));
        }
        q(&Value::Array(list).to_string())
    }

    pub fn mount(mut self: Pin<&mut Self>, device: &QString) {
        let device = device.to_string();
        self.as_mut().set_busy(true);
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let (mp, err) = match media::mount(&device) {
                Ok(mp) => (mp, String::new()),
                Err(e) => (String::new(), e),
            };
            qt_thread
                .queue(move |mut o| {
                    o.as_mut().set_busy(false);
                    o.as_mut().mounted(&q(&device), &q(&mp), &q(&err));
                })
                .ok();
        });
    }

    pub fn apply(mut self: Pin<&mut Self>, choices: &QString) {
        let choices: Value = serde_json::from_str(&choices.to_string()).unwrap_or(Value::Null);
        self.as_mut().set_busy(true);
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = apply_choices(&choices);
            qt_thread
                .queue(move |mut o| {
                    o.as_mut().set_busy(false);
                    let (ok, err) = match result {
                        Ok(()) => (true, String::new()),
                        Err(e) => (false, e),
                    };
                    o.as_mut().applied(ok, &q(&err));
                })
                .ok();
        });
    }

    pub fn connect_share(mut self: Pin<&mut Self>, share: &QString) {
        let v: Value = serde_json::from_str(&share.to_string()).unwrap_or(Value::Null);
        let field = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
        let (kind, server, name, user) = (field("kind"), field("server"), field("share"), field("user"));
        let password = v["password"].as_str().unwrap_or("").to_string();
        if let Err(code) = check_share(&kind, &server, &name, &user, &password) {
            self.as_mut().share_changed(false, &q(code), &q(""));
            return;
        }
        self.as_mut().set_busy(true);
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let mut args = vec!["nas-add", "--kind", kind.as_str(), "--server", server.as_str(), "--share", name.as_str()];
            if kind == "smb" && !user.is_empty() {
                args.extend(["--user", user.as_str()]);
            }
            let result = run_helper(&args, &format!("{password}\n"));
            qt_thread
                .queue(move |mut o| {
                    o.as_mut().set_busy(false);
                    match result {
                        Ok(path) => o.as_mut().share_changed(true, &q(""), &q(&path)),
                        Err(e) => o.as_mut().share_changed(false, &q(&e), &q("")),
                    }
                })
                .ok();
        });
    }

    pub fn remove_share(mut self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        let id = path.strip_prefix(&format!("{NAS_BASE}/")).unwrap_or("").to_string();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            self.as_mut().share_changed(false, &q("share"), &q(""));
            return;
        }
        self.as_mut().set_busy(true);
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = run_helper(&["nas-remove", "--id", &id], "");
            qt_thread
                .queue(move |mut o| {
                    o.as_mut().set_busy(false);
                    match result {
                        Ok(_) => o.as_mut().share_changed(true, &q(""), &q("")),
                        Err(e) => o.as_mut().share_changed(false, &q(&e), &q("")),
                    }
                })
                .ok();
        });
    }

    pub fn set_schedule(&self, on: bool) -> QString {
        let args: &[&str] = if on {
            &["enable", "--now", profile::TIMER]
        } else {
            &["disable", "--now", profile::TIMER]
        };
        let (ok, text) = systemctl_user(args);
        q(if ok { "" } else { &text })
    }

    pub fn schedule(&self) -> QString {
        let installed = systemctl_user(&["cat", profile::TIMER]).0;
        let enabled = systemctl_user(&["is-enabled", profile::TIMER]).1 == "enabled";
        let next = systemctl_user(&["show", profile::TIMER, "-p", "NextElapseUSecRealtime", "--value", "--timestamp=unix"])
            .1
            .trim_start_matches('@')
            .parse::<u64>()
            .unwrap_or(0);
        q(&json!({ "installed": installed, "enabled": enabled, "next": next }).to_string())
    }

    pub fn status(&self) -> QString {
        let mut status = profile::read_status();
        status["active"] = json!(backup_active());
        q(&status.to_string())
    }

    pub fn run_now(&self) -> QString {
        if backup_active() {
            return q("running");
        }
        let Some(tool) = backup_tool() else { return q("missing") };
        let log = profile::status_path().with_file_name("data-backup-last.log");
        if let Some(dir) = log.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let out = std::fs::File::create(&log).ok();
        let mut cmd = Command::new(tool);
        cmd.args(["--data-backup", "--manual"])
            .stdin(Stdio::null())
            // Its own process group: it goes on when the window closes
            .process_group(0);
        match out.and_then(|f| f.try_clone().ok().map(|g| (f, g))) {
            Some((f, g)) => {
                cmd.stdout(f).stderr(g);
            }
            None => {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        match cmd.spawn() {
            Ok(mut child) => {
                // Reaped in the background: no zombie
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                q("started")
            }
            Err(_) => q("missing"),
        }
    }

    pub fn check_folder(&self, path: &QString) -> QString {
        let p = PathBuf::from(path.to_string().trim().trim_end_matches('/'));
        let homes = Homes::current();
        if !p.is_absolute() || !p.is_dir() {
            return q("missing");
        }
        // The whole home or the system would also copy the backup and caches
        if p.components().count() <= 2 || p == homes.home {
            return q("too_wide");
        }
        if Path::new(profile::INTERNAL_BASE).starts_with(&p) || p.starts_with(profile::INTERNAL_BASE) {
            return q("destination");
        }
        q("")
    }

    pub fn system_config(&self) -> QString {
        let c = crate::config::ConfigManager::load().unwrap_or_default();
        q(&json!({ "dest": c.dest_path, "keep": c.keep_clamped() }).to_string())
    }

    pub fn save_system_excluded(&self, paths: &QString) -> QString {
        let mut c = crate::config::ConfigManager::load().unwrap_or_default();
        c.excluded_paths = paths.to_string().lines().map(str::trim).filter(|p| p.starts_with('/')).map(String::from).collect();
        match crate::config::ConfigManager::save(&c) {
            Ok(()) => q(""),
            Err(e) => q(&e.to_string()),
        }
    }

    pub fn offered_drives(&self, media: &QString) -> QString {
        let list: Vec<Value> = serde_json::from_str(&media.to_string()).unwrap_or_default();
        q(&Value::Array(list.into_iter().filter(drives::offered).collect()).to_string())
    }

    pub fn home_folders(&self) -> QString {
        let homes = Homes::current();
        let list: Vec<String> = profile::subfolders(&homes.home)
            .into_iter()
            .filter(|p| !p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true))
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        q(&json!(list).to_string())
    }
}

fn free_of(path: &Path) -> u64 {
    storage::available_space(&storage::existing_ancestor(path)).unwrap_or(0)
}

/// Destination folder, saved choices, Syncthing folder, timer: in this order, so a step
/// that fails leaves the previous setup working
fn apply_choices(choices: &Value) -> Result<(), String> {
    let dest: Destination = serde_json::from_value(choices["destination"].clone()).map_err(|_| "destination".to_string())?;
    if dest.path.is_empty() {
        return Err("destination".into());
    }
    let path = Path::new(&dest.path);

    // The folder: created by the user on a share, by the root helper elsewhere (once)
    if !(path.is_dir() && writable(path)) {
        if dest.kind == "nas" {
            // Only on a share that is mounted now: else the folder would be on the SD card
            let share = dest.path.strip_suffix(&format!("/{}", profile::MEDIA_SUBDIR)).unwrap_or("");
            // A share added here is mounted when used: this mounts it
            let _ = std::fs::read_dir(share);
            if !network_mounts().iter().any(|(target, _, _)| target == share) {
                return Err("The network folder is not mounted.".into());
            }
            std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
        } else {
            let helper = storage::helper_path().ok_or("Helper script not found (astroarch-backup-helper).")?;
            let out = Command::new("pkexec")
                .arg(helper)
                .args(["datadir", "--dest", &dest.path])
                .stdin(Stdio::null())
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(match out.status.code() {
                    Some(126) | Some(127) => "auth".into(),
                    _ => String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .find_map(|l| l.strip_prefix("ERROR:").map(|r| r.splitn(2, ':').nth(1).unwrap_or(r).to_string()))
                        .unwrap_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_string()),
                });
            }
        }
    }

    let previous = profile::load_config();
    let strings = |v: &Value| -> Vec<String> {
        v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default()
    };
    let config = Config {
        excluded: strings(&choices["excluded"]),
        added: strings(&choices["added"]),
        keep: choices["keep"].as_u64().map(|k| k as u32).unwrap_or(profile::DEFAULT_KEEP).clamp(1, 100),
        // The same subfolder as before: a new one would start the history again
        id: previous.map(|c| c.id).filter(|id| !id.is_empty()).unwrap_or_else(profile::new_id),
        destination: dest.clone(),
    };
    profile::save_config(&config).map_err(|e| e.to_string())?;

    // Syncthing: the folder is sent from the Pi only; versions are kept on the computer
    if dest.kind == "syncthing" {
        let folder = path.join(&config.id);
        let _ = std::fs::create_dir_all(&folder);
        let out = Command::new("syncthing")
            .args(["cli", "config", "folders", "add", "--id", "astroarch-data-backup", "--label", "AstroArch backup",
                   "--type", "sendonly", "--path"])
            .arg(&folder)
            .output()
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&out.stderr).to_string();
        if !out.status.success() && !text.contains("exists") {
            return Err(format!("syncthing: {}", text.trim()));
        }
    }

    if choices["schedule"].as_bool().unwrap_or(true) {
        let (ok, text) = systemctl_user(&["enable", "--now", profile::TIMER]);
        if !ok {
            return Err(format!("timer: {text}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_share;

    #[test]
    fn share_rules() {
        assert!(check_share("smb", "nas.local", "Backup Disk", "teo", "p@ss w,rd").is_ok());
        assert!(check_share("smb", "192.168.1.10", "astro$", "", "").is_ok());
        assert_eq!(check_share("smb", "-o", "x", "", ""), Err("server"));
        assert_eq!(check_share("smb", "nas;rm", "x", "", ""), Err("server"));
        assert_eq!(check_share("smb", "nas", "a/../b", "", ""), Err("share"));
        assert_eq!(check_share("smb", "nas", "x,uid=0", "", ""), Err("share"));
        assert_eq!(check_share("smb", "nas", "x", "a,b", ""), Err("user"));
        assert_eq!(check_share("smb", "nas", "x", "u", "a\nb"), Err("password"));
        assert!(check_share("nfs", "nas", "/export/astro", "", "").is_ok());
        assert_eq!(check_share("nfs", "nas", "export", "", ""), Err("export"));
        assert_eq!(check_share("nfs", "nas", "/a/../etc", "", ""), Err("export"));
    }
}

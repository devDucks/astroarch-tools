#![allow(dead_code)]

//! "Data and settings" backup: what it offers (data/data-profile.toml), the user's choices
//! (~/.config/astroarch/data-backup.json) and the backup itself, run as the user every
//! 2 hours by astroarch-data-backup.timer (`astroarch-backup --data-backup`).
//!
//! No root here: the destination folder is created once by the root helper
//! (`astroarch-backup-helper datadir`), then owned by the user.
//!
//! Layout in the destination, one folder per system (`id`, so a new install or a clone
//! never rotates away the backups of another one):
//!   <dest>/<id>/settings/snapshot-YYYYMMDD-HHMMSS/<absolute path>  settings with history;
//!       unchanged files are hard links to the previous snapshot. With Syncthing there is
//!       one copy, settings/current/, and the computer keeps the versions
//!   <dest>/<id>/data/<absolute path>  images and other data, only added to
//!
//! The Control Center (src/data_backup.rs there) compiles this same file in.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The catalog, compiled in: the paths are the same on every AstroArch
const CATALOG: &str = include_str!("../data/data-profile.toml");

/// Snapshots of the settings kept by default (one day and a bit, every 2 hours)
pub const DEFAULT_KEEP: u32 = 14;
pub const TIMER: &str = "astroarch-data-backup.timer";
/// Destination on the main disk: /var/backups is never in the system backup
pub const INTERNAL_BASE: &str = "/var/backups/astroarch-data";
/// Folder on a drive or a network share
pub const MEDIA_SUBDIR: &str = "astroarch/data-backup";

// ------------------------------------------------------------------------ catalog

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub group: Vec<Group>,
    pub item: Vec<Item>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Group {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Settings,
    Data,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub id: String,
    pub group: String,
    pub name: String,
    pub paths: Vec<String>,
    pub kind: Kind,
    #[serde(default)]
    pub essential: bool,
    #[serde(default = "yes")]
    pub default: bool,
    #[serde(default)]
    pub warn: String,
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub needed_by: String,
    #[serde(default)]
    pub subfolders: bool,
    #[serde(default)]
    pub skip: Vec<String>,
    #[serde(default)]
    pub child: Vec<Child>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Child {
    pub id: String,
    pub name: String,
    pub paths: Vec<String>,
    pub kind: Option<Kind>,
    #[serde(default = "yes")]
    pub default: bool,
    #[serde(default)]
    pub note: String,
}

pub fn catalog() -> Catalog {
    toml::from_str(CATALOG).expect("data/data-profile.toml is valid")
}

/// The homes that "~/" and "~kiosk/" stand for
#[derive(Debug, Clone)]
pub struct Homes {
    pub user: String,
    pub home: PathBuf,
    pub kiosk: Option<PathBuf>,
}

impl Homes {
    pub fn current() -> Homes {
        let user = std::env::var("USER")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| run_text("id", &["-un"]).unwrap_or_default());
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        // The kiosk user of this user (scripts/astroarch-kiosk-user.sh in the astroarch repo)
        let kiosk = run_text("getent", &["passwd", &format!("{user}-kiosk")])
            .and_then(|line| line.split(':').nth(5).map(PathBuf::from))
            .filter(|h| h.is_dir());
        Homes { user, home, kiosk }
    }

    /// Absolute path, or None for a kiosk path when there is no kiosk user
    pub fn expand(&self, path: &str) -> Option<PathBuf> {
        let join = |base: &Path, rest: &str| {
            let rest = rest.trim_end_matches('/');
            if rest.is_empty() {
                base.to_path_buf()
            } else {
                base.join(rest)
            }
        };
        if let Some(rest) = path.strip_prefix("~kiosk/") {
            self.kiosk.as_ref().map(|k| join(k, rest))
        } else if let Some(rest) = path.strip_prefix("~/") {
            Some(join(&self.home, rest))
        } else {
            Some(PathBuf::from(path))
        }
    }

    pub fn expand_all(&self, paths: &[String]) -> Option<Vec<PathBuf>> {
        paths.iter().map(|p| self.expand(p)).collect()
    }
}

// ------------------------------------------------------------------------- config

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Destination {
    /// "internal", "usb", "nas" or "syncthing"
    pub kind: String,
    /// The folder that holds <id>/ (created at setup, never by a backup)
    pub path: String,
    /// What the user saw: the drive's label, the share...
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Item ids, "item/child", or absolute paths of unticked subfolders
    pub excluded: Vec<String>,
    /// Folders added by the user (data, only added to)
    pub added: Vec<String>,
    pub destination: Destination,
    pub keep: u32,
    /// Subfolder of this system in the destination
    pub id: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            excluded: Vec::new(),
            added: Vec::new(),
            destination: Destination::default(),
            keep: DEFAULT_KEEP,
            id: String::new(),
        }
    }
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())).join(fallback))
}

pub fn config_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("astroarch/data-backup.json")
}

pub fn status_path() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("astroarch/data-backup.json")
}

/// Held by a running backup
pub fn lock_path() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("astroarch/data-backup.lock")
}

/// None when the backup was never set up
pub fn load_config() -> Option<Config> {
    let text = std::fs::read_to_string(config_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// Exclusions before the first setup: what the catalog leaves unticked
pub fn default_excluded(catalog: &Catalog) -> Vec<String> {
    let mut out = Vec::new();
    for item in &catalog.item {
        if !item.default {
            out.push(item.id.clone());
        }
        for c in &item.child {
            if !c.default {
                out.push(format!("{}/{}", item.id, c.id));
            }
        }
    }
    out
}

/// Written to a temporary file then renamed, so an interrupted save never leaves half a file
fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value)?)?;
    std::fs::rename(&tmp, path)
}

pub fn save_config(config: &Config) -> std::io::Result<()> {
    write_json(&config_path(), &serde_json::to_value(config)?)
}

/// A new subfolder name for this system: hostname, date, random part (as setup.sh of the
/// first version did)
pub fn new_id() -> String {
    let host = std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "astroarch".into());
    let date = run_text("date", &["+%Y%m%d"]).unwrap_or_default();
    let random = std::fs::read("/proc/sys/kernel/random/uuid")
        .map(|u| String::from_utf8_lossy(&u[..4.min(u.len())]).to_string())
        .unwrap_or_default();
    format!("{host}-{date}-{random}")
}

// --------------------------------------------------------------------------- plan

/// What one run copies
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub settings: Vec<PathBuf>,
    pub data: Vec<PathBuf>,
    /// Never copied, even inside a source (absolute)
    pub excludes: Vec<PathBuf>,
}

/// Is `key` ticked? An item excluded as a whole excludes its children
fn ticked(excluded: &BTreeSet<&str>, key: &str) -> bool {
    !excluded.contains(key)
}

/// Is this item in the backup? `needed_by` keeps it in while its item is ticked
pub fn item_included(catalog: &Catalog, excluded: &BTreeSet<&str>, item: &Item) -> bool {
    if ticked(excluded, &item.id) {
        return true;
    }
    !item.needed_by.is_empty()
        && catalog
            .item
            .iter()
            .any(|other| other.id == item.needed_by && ticked(excluded, &other.id))
}

pub fn plan(catalog: &Catalog, config: &Config, homes: &Homes) -> Plan {
    let excluded: BTreeSet<&str> = config.excluded.iter().map(String::as_str).collect();
    let mut plan = Plan::default();
    let add = |kind: Kind, path: PathBuf, plan: &mut Plan| {
        let list = if kind == Kind::Settings { &mut plan.settings } else { &mut plan.data };
        if !list.contains(&path) {
            list.push(path);
        }
    };

    for item in &catalog.item {
        // Kiosk paths without a kiosk user: nothing to back up
        let Some(paths) = homes.expand_all(&item.paths) else { continue };
        for skip in &item.skip {
            if let Some(p) = homes.expand(skip) {
                plan.excludes.push(p);
            }
        }
        let included = item_included(catalog, &excluded, item);
        if item.child.is_empty() {
            if included {
                for p in paths {
                    add(item.kind, p, &mut plan);
                }
            } else {
                // Unticked, but maybe inside another source
                plan.excludes.extend(paths);
            }
            continue;
        }
        for child in &item.child {
            let Some(child_paths) = homes.expand_all(&child.paths) else { continue };
            let key = format!("{}/{}", item.id, child.id);
            if included && ticked(&excluded, &key) {
                for p in child_paths {
                    add(child.kind.unwrap_or(item.kind), p, &mut plan);
                }
            } else {
                plan.excludes.extend(child_paths);
            }
        }
    }

    // Unticked subfolders and the folders the user added
    for entry in &config.excluded {
        if entry.starts_with('/') {
            plan.excludes.push(PathBuf::from(entry));
        }
    }
    for added in &config.added {
        if added.starts_with('/') {
            add(Kind::Data, PathBuf::from(added), &mut plan);
        }
    }
    plan.excludes.sort();
    plan.excludes.dedup();
    plan
}

/// rsync filter for an absolute path. With --relative the transfer names are the full
/// paths, so a leading "/" anchors it. Wildcards in a real name are escaped (a backslash
/// is only special in a pattern that has wildcards)
pub fn rsync_exclude(path: &Path) -> String {
    let p = path.to_string_lossy();
    let pattern = if p.contains(['*', '?', '[']) {
        p.chars()
            .flat_map(|c| match c {
                '*' | '?' | '[' | ']' | '\\' => vec!['\\', c],
                _ => vec![c],
            })
            .collect()
    } else {
        p.to_string()
    };
    format!("--exclude={pattern}")
}

// ------------------------------------------------------------------- page rows

/// Rows of the selection list for the Control Center page: groups, items, children (fixed
/// or the real subfolders), with what exists. Sizes come later (see `sizes`)
pub fn rows(catalog: &Catalog, homes: &Homes) -> Value {
    let display = |p: &PathBuf| -> String {
        let s = p.to_string_lossy().to_string();
        let home = homes.home.to_string_lossy().to_string();
        match s.strip_prefix(&home) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
            _ => s,
        }
    };
    let groups: Vec<Value> = catalog
        .group
        .iter()
        .map(|g| {
            let items: Vec<Value> = catalog
                .item
                .iter()
                .filter(|i| i.group == g.id)
                .filter_map(|item| {
                    let paths = homes.expand_all(&item.paths)?;
                    let mut children: Vec<Value> = item
                        .child
                        .iter()
                        .filter_map(|c| {
                            let cp = homes.expand_all(&c.paths)?;
                            Some(json!({
                                "key": format!("{}/{}", item.id, c.id),
                                "name": c.name,
                                "paths": cp.iter().map(display).collect::<Vec<_>>(),
                                "exists": cp.iter().any(|p| p.exists()),
                                "note": c.note,
                            }))
                        })
                        .collect();
                    if item.subfolders {
                        if let Some(base) = paths.first() {
                            children = subfolders(base)
                                .into_iter()
                                .map(|p| {
                                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                    json!({ "key": p.to_string_lossy(), "name": name,
                                            "paths": [display(&p)], "exists": true, "note": "" })
                                })
                                .collect();
                        }
                    }
                    Some(json!({
                        "id": item.id,
                        "name": item.name,
                        "paths": paths.iter().map(display).collect::<Vec<_>>(),
                        "exists": paths.iter().any(|p| p.exists()),
                        "essential": item.essential,
                        "warn": item.warn,
                        "hint": item.hint,
                        "neededBy": item.needed_by,
                        "subfolders": item.subfolders,
                        "children": children,
                    }))
                })
                .collect();
            json!({ "id": g.id, "title": g.title, "items": items })
        })
        .filter(|g| g["items"].as_array().is_some_and(|a| !a.is_empty()))
        .collect();
    Value::Array(groups)
}

/// Real subfolders (not links), sorted without regard to case
pub fn subfolders(base: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(base)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    out
}

/// Bytes of the paths (apparent size, links not followed); unreadable parts count as 0
pub fn size_of(paths: &[PathBuf]) -> u64 {
    let existing: Vec<&PathBuf> = paths.iter().filter(|p| p.exists() || p.is_symlink()).collect();
    if existing.is_empty() {
        return 0;
    }
    let out = Command::new("du").args(["-scb", "--"]).args(&existing).output();
    out.ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .last()
                .and_then(|l| l.split_whitespace().next().and_then(|n| n.parse().ok()))
        })
        .unwrap_or(0)
}

/// (key, paths) of every row, for the sizes shown in the list
pub fn size_keys(catalog: &Catalog, homes: &Homes) -> Vec<(String, Vec<PathBuf>)> {
    let mut out = Vec::new();
    for item in &catalog.item {
        let Some(paths) = homes.expand_all(&item.paths) else { continue };
        if item.subfolders {
            for p in paths.first().map(|b| subfolders(b)).unwrap_or_default() {
                out.push((p.to_string_lossy().to_string(), vec![p]));
            }
            // Loose files at the top: the item's own size minus its subfolders (QML adds)
            out.push((item.id.clone(), paths));
        } else if item.child.is_empty() {
            out.push((item.id.clone(), paths));
        } else {
            for c in &item.child {
                if let Some(cp) = homes.expand_all(&c.paths) {
                    out.push((format!("{}/{}", item.id, c.id), cp));
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------------- status

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn read_status() -> Value {
    std::fs::read_to_string(status_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}))
}

/// state: running | ok | partial | failed | skipped. "lastOk" is kept across runs: the last
/// COMPLETE backup (a partial one left files out)
fn write_status(state: &str, fields: Value) {
    let previous = read_status();
    let mut status = json!({ "state": state, "time": now(), "lastOk": previous["lastOk"] });
    if let (Some(map), Some(extra)) = (status.as_object_mut(), fields.as_object()) {
        for (k, v) in extra {
            map.insert(k.clone(), v.clone());
        }
    }
    if state == "ok" {
        status["lastOk"] = json!(now());
    }
    let _ = write_json(&status_path(), &status);
}

// ------------------------------------------------------------------------- engine

/// A session is in progress while KStars and indiserver both run (any user)
pub fn imaging() -> bool {
    let mut kstars = false;
    let mut indi = false;
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            if let Ok(comm) = std::fs::read_to_string(e.path().join("comm")) {
                match comm.trim() {
                    "kstars" => kstars = true,
                    "indiserver" => indi = true,
                    _ => {}
                }
            }
        }
    }
    kstars && indi
}

fn run_text(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// rsync at low priority. Files that vanish meanwhile (24) are fine, files it cannot read
/// (23) are reported without stopping the rest
fn rsync(args: &[String]) -> Result<bool, String> {
    let mut cmd = Command::new("nice");
    cmd.args(["-n", "10", "ionice", "-c", "3", "rsync"]).args(args);
    println!("rsync {}", args.join(" "));
    let status = cmd.status().map_err(|e| format!("cannot start rsync: {e}"))?;
    match status.code() {
        Some(0) | Some(24) => Ok(true),
        Some(23) => Ok(false),
        code => Err(format!("rsync failed ({})", code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into()))),
    }
}

fn existing(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| p.exists() || p.is_symlink())
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

/// One backup. `manual`: asked by the user, so it runs during a session too.
/// Exit code: 0 done or skipped, 1 failed or some files left out
pub fn run(manual: bool) -> i32 {
    let Some(config) = load_config() else {
        eprintln!("The data backup is not set up: open the Control Center, Data backup");
        return 1;
    };
    if !manual && imaging() {
        println!("KStars and INDI are running, a session is in progress: skipped");
        write_status("skipped", json!({ "reason": "imaging" }));
        return 0;
    }

    // One run at a time: a run by hand and one by the timer would clean up each other's snapshot
    let lock = lock_path();
    if let Some(dir) = lock.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(lock_file) = std::fs::File::create(&lock) else {
        eprintln!("Cannot create {}", lock.display());
        return 1;
    };
    // The Control Center looks at the lock for an instant to tell whether a backup runs:
    // a few tries, so a timer run starting at that moment is not skipped
    let mut locked = false;
    for _ in 0..10 {
        if lock_file.try_lock().is_ok() {
            locked = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if !locked {
        println!("Another backup is running: skipped");
        return 0;
    }

    let fail = |reason: &str, message: String| {
        eprintln!("{message}");
        write_status("failed", json!({ "reason": reason, "message": message }));
        1
    };

    let base = PathBuf::from(&config.destination.path);
    // Never create the base folder: with the drive unplugged or the NAS off, the backup
    // would end up on the SD card
    if config.destination.path.is_empty() || !base.is_dir() {
        return fail("missing", format!("{} is not there: is the drive plugged in, is the NAS on?", base.display()));
    }
    // A drive or a share that is not mounted: its folder would be on the system disk
    if matches!(config.destination.kind.as_str(), "usb" | "nas") && same_device(&base, Path::new("/")) {
        return fail("missing", format!("{} is not mounted: is the drive plugged in, is the NAS on?", base.display()));
    }
    if config.id.is_empty() {
        return fail("config", "No backup ID in the configuration".into());
    }
    let homes = Homes::current();
    let catalog = catalog();
    let plan = plan(&catalog, &config, &homes);
    // A backup inside what it copies would copy itself
    if let Some(src) = plan.settings.iter().chain(&plan.data).find(|s| base.starts_with(s)) {
        return fail("inside", format!("{} is inside {}: choose a folder outside it", base.display(), src.display()));
    }
    let root = base.join(&config.id);
    let settings_dir = root.join("settings");
    let data_dir = root.join("data");
    // rsync writes each file in tmp/ then moves it in place: a run cut short (power cut,
    // systemctl stop) leaves its half files there, and each run starts with it empty
    let tmp_dir = root.join("tmp");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    for d in [&settings_dir, &data_dir, &tmp_dir] {
        if let Err(e) = std::fs::create_dir_all(d) {
            return fail("write", format!("Cannot write to {}: {e}", d.display()));
        }
    }
    let temp_dir = format!("--temp-dir={}", tmp_dir.display());
    let excludes: Vec<String> = plan.excludes.iter().map(|p| rsync_exclude(p)).collect();
    // On a network share the owner and the permissions are those of the mount (uid=,
    // file_mode=): compared, they would differ for every file, and --link-dest would copy
    // each snapshot whole. There, times and sizes only
    let copy_mode = if config.destination.kind == "nas" { "-rltR" } else { "-aR" };
    let mut complete = true;
    write_status("running", json!({ "stage": "settings", "started": now() }));

    // Settings: a snapshot built under a temporary name, so one cut short is never taken
    // as the last complete one; with Syncthing one copy (the computer keeps versions)
    let sources = existing(&plan.settings);
    let mut snapshot = String::new();
    if !sources.is_empty() {
        if config.destination.kind == "syncthing" {
            let mut args: Vec<String> = vec![copy_mode.into(), temp_dir.clone(), "--delete".into()];
            args.extend(excludes.iter().cloned());
            args.extend(sources);
            args.push(format!("{}/current/", settings_dir.display()));
            match rsync(&args) {
                Ok(ok) => complete &= ok,
                Err(e) => return fail("rsync", e),
            }
        } else {
            let mut snaps = snapshots(&settings_dir);
            for old in std::fs::read_dir(&settings_dir).into_iter().flatten().flatten() {
                if old.file_name().to_string_lossy().starts_with("incomplete-") {
                    let _ = std::fs::remove_dir_all(old.path());
                }
            }
            let date = run_text("date", &["+%Y%m%d-%H%M%S"]).unwrap_or_default();
            let stamp = next_stamp(snaps.last().map(String::as_str), &date);
            let work = settings_dir.join(format!("incomplete-{stamp}"));
            let mut args: Vec<String> = vec![copy_mode.into(), temp_dir.clone()];
            // Files that did not change are hard links to the previous snapshot
            if let Some(prev) = snaps.last() {
                args.push(format!("--link-dest={}", settings_dir.join(prev).display()));
            }
            args.extend(excludes.iter().cloned());
            args.extend(sources);
            args.push(format!("{}/", work.display()));
            match rsync(&args) {
                Ok(ok) => complete &= ok,
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&work);
                    return fail("rsync", e);
                }
            }
            snapshot = format!("snapshot-{stamp}");
            if let Err(e) = std::fs::rename(&work, settings_dir.join(&snapshot)) {
                return fail("write", format!("Cannot finish the snapshot: {e}"));
            }
            snaps.push(snapshot.clone());
            let keep = config.keep.clamp(1, 100) as usize;
            for old in snaps.iter().take(snaps.len().saturating_sub(keep)) {
                let _ = std::fs::remove_dir_all(settings_dir.join(old));
            }
            println!("Snapshot {snapshot}, {} kept", snaps.len().min(keep));
        }
    }

    // Data is only added to: a file deleted on the Pi stays in the backup
    write_status("running", json!({ "stage": "data", "started": now() }));
    let sources = existing(&plan.data);
    if !sources.is_empty() {
        let mut args: Vec<String> = vec![copy_mode.into(), temp_dir.clone()];
        args.extend(excludes.iter().cloned());
        args.extend(sources);
        args.push(format!("{}/", data_dir.display()));
        match rsync(&args) {
            Ok(ok) => complete &= ok,
            Err(e) => return fail("rsync", e),
        }
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    let state = if complete { "ok" } else { "partial" };
    write_status(state, json!({ "snapshot": snapshot, "folder": root.to_string_lossy() }));
    // A partial backup still succeeds (the status file says what was left out): the timer
    // unit would otherwise show as failed every 2 hours
    if complete {
        println!("Backup done");
    } else {
        println!("Backup done, but some files could not be read and were left out");
    }
    0
}

/// Name of the new snapshot: the date, unless it does not come after the last snapshot
/// (two runs in the same second, or a Pi without network nor clock that started months
/// behind): then the last name with a counter, so the newest always sorts last and the
/// rotation never removes it
pub fn next_stamp(last: Option<&str>, date: &str) -> String {
    let Some(last) = last.and_then(|l| l.strip_prefix("snapshot-")) else {
        return if date.is_empty() { "00000000-000000".into() } else { date.into() };
    };
    if !date.is_empty() && date > last && !last.starts_with(date) {
        return date.into();
    }
    // 20261029-220554 -> 20261029-220554.0001 -> .0002 ...
    let (base, count) = match last.rsplit_once('.') {
        Some((b, n)) if n.len() == 4 && n.chars().all(|c| c.is_ascii_digit()) => (b, n.parse::<u32>().unwrap_or(0)),
        _ => (last, 0),
    };
    format!("{base}.{:04}", count + 1)
}

fn same_device(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev(),
        _ => false,
    }
}

/// Snapshot names, oldest first
pub fn snapshots(settings_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(settings_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("snapshot-"))
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn homes(kiosk: bool) -> Homes {
        Homes {
            user: "astronaut".into(),
            home: PathBuf::from("/home/astronaut"),
            kiosk: kiosk.then(|| PathBuf::from("/home/astronaut-kiosk")),
        }
    }

    fn config(excluded: &[&str]) -> Config {
        Config { excluded: excluded.iter().map(|s| s.to_string()).collect(), ..Config::default() }
    }

    #[test]
    fn catalog_is_valid() {
        let c = catalog();
        let ids: BTreeSet<&str> = c.item.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids.len(), c.item.len(), "item ids are unique");
        for item in &c.item {
            assert!(c.group.iter().any(|g| g.id == item.group), "group of {}", item.id);
            assert!(item.needed_by.is_empty() || ids.contains(item.needed_by.as_str()));
            assert!(!item.subfolders || item.paths.len() == 1);
        }
    }

    #[test]
    fn defaults_leave_out_index_files_and_cache() {
        let c = catalog();
        let d = default_excluded(&c);
        assert!(d.contains(&"kstars-astrometry".to_string()));
        assert!(d.contains(&"kiosk/cache".to_string()));
        let p = plan(&c, &config(&d.iter().map(String::as_str).collect::<Vec<_>>()), &homes(true));
        assert!(p.settings.contains(&PathBuf::from("/home/astronaut/.indi")));
        assert!(p.data.contains(&PathBuf::from("/srv/astro-data")));
        assert!(p.excludes.contains(&PathBuf::from("/home/astronaut/.local/share/kstars/astrometry")));
        assert!(p.excludes.contains(&PathBuf::from("/home/astronaut-kiosk/.cache")));
        assert!(p.excludes.contains(&PathBuf::from("/home/astronaut-kiosk/.local/state")));
    }

    #[test]
    fn dark_library_keeps_the_database() {
        let c = catalog();
        let db = PathBuf::from("/home/astronaut/.local/share/kstars/userdb.sqlite");
        let p = plan(&c, &config(&["kstars-db"]), &homes(false));
        assert!(p.settings.contains(&db));
        let p = plan(&c, &config(&["kstars-db", "kstars-darks"]), &homes(false));
        assert!(!p.settings.contains(&db));
        assert!(p.excludes.contains(&db));
    }

    #[test]
    fn no_kiosk_user_no_kiosk_paths() {
        let p = plan(&catalog(), &config(&[]), &homes(false));
        assert!(p.settings.iter().chain(&p.data).all(|s| !s.starts_with("/home/astronaut-kiosk")));
    }

    #[test]
    fn unticked_parent_excludes_children_and_subfolders_are_excludes() {
        let c = catalog();
        let p = plan(&c, &config(&["kiosk", "/srv/astro-data/Dark"]), &homes(true));
        assert!(!p.settings.iter().any(|s| s.starts_with("/home/astronaut-kiosk")));
        assert!(p.excludes.contains(&PathBuf::from("/home/astronaut-kiosk/.indi")));
        assert!(p.excludes.contains(&PathBuf::from("/srv/astro-data/Dark")));
        let p = plan(&c, &config(&["kiosk/kstars"]), &homes(true));
        // .config is copied by "apps", kstarsrc of the kiosk stays out
        assert!(p.settings.contains(&PathBuf::from("/home/astronaut-kiosk/.config")));
        assert!(p.excludes.contains(&PathBuf::from("/home/astronaut-kiosk/.config/kstarsrc")));
        assert!(p.data.contains(&PathBuf::from("/home/astronaut-kiosk/Pictures")));
    }

    #[test]
    fn added_folders_are_data() {
        let mut cfg = config(&[]);
        cfg.added = vec!["/home/astronaut/Documents".into()];
        let p = plan(&catalog(), &cfg, &homes(false));
        assert!(p.data.contains(&PathBuf::from("/home/astronaut/Documents")));
    }

    #[test]
    fn exclude_patterns() {
        assert_eq!(rsync_exclude(Path::new("/srv/astro-data/Dark")), "--exclude=/srv/astro-data/Dark");
        assert_eq!(rsync_exclude(Path::new("/srv/a [1]")), "--exclude=/srv/a \\[1\\]");
        assert_eq!(rsync_exclude(Path::new("/srv/M'sChain")), "--exclude=/srv/M'sChain");
    }

    #[test]
    fn snapshot_names_always_sort_last() {
        assert_eq!(next_stamp(None, "20261029-220554"), "20261029-220554");
        assert_eq!(next_stamp(Some("snapshot-20261029-200000"), "20261029-220554"), "20261029-220554");
        // Same second, or a clock behind the last snapshot
        assert_eq!(next_stamp(Some("snapshot-20261029-220554"), "20261029-220554"), "20261029-220554.0001");
        assert_eq!(next_stamp(Some("snapshot-20261029-220554"), "20250101-000100"), "20261029-220554.0001");
        assert_eq!(next_stamp(Some("snapshot-20261029-220554.0009"), "20250101-000100"), "20261029-220554.0010");
        // The clock is right again
        assert_eq!(next_stamp(Some("snapshot-20261029-220554.0002"), "20261030-000000"), "20261030-000000");
        let mut names = vec!["snapshot-20261029-220554".to_string(), "snapshot-20261029-220554.0001".to_string(),
                             "snapshot-20261029-220554.0002".to_string(), "snapshot-20261030-000000".to_string()];
        let sorted = names.clone();
        names.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn expand_paths() {
        let h = homes(true);
        assert_eq!(h.expand("~/.indi"), Some(PathBuf::from("/home/astronaut/.indi")));
        assert_eq!(h.expand("~kiosk/"), Some(PathBuf::from("/home/astronaut-kiosk")));
        assert_eq!(homes(false).expand("~kiosk/.indi"), None);
        assert_eq!(h.expand("/srv/astro-data"), Some(PathBuf::from("/srv/astro-data")));
    }
}

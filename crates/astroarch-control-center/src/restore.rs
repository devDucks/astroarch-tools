//! QObject of the "Restore from a backup" page (qml/RestoreView.qml).
//!
//! Four sources, one model: a folder that holds the files of another (or this) AstroArch
//! at their absolute paths, e.g. <root>/home/astronaut/.indi.
//!   - a snapshot of the data backup: <dest>/<id>/settings/snapshot-DATE (settings) and
//!     <dest>/<id>/data (images), as data_profile.rs writes them
//!   - an old AstroArch disk, mounted read-only by the root helper under /run/astroarch-cc-restore
//!   - the AstroMonitor backup, downloaded from its server and laid out the same way
//!   - a snapshot of the system backup (the backup tool's btrfs image), mounted read-only by
//!     its own helper (BackupManager.browse): <browse mount>/snapshots/<name>. Even one of
//!     another architecture (a Raspberry Pi's on a PC): only settings and data are copied
//!
//! What can be restored is the catalog of the data backup (data/data-profile.toml of the
//! backup tool, compiled in): the same rows, the same plan, read in the source instead of
//! the system. The copy is done by scripts/astroarch-restore-helper.sh with pkexec: root
//! reads, the user (or its kiosk user) writes, and what was there is moved aside.

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::data_profile::{self as profile, present, Catalog, Config, Homes, Kind};
use crate::media;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, busy)]
        // While busy: "auth" (waiting for the password), "mount", "fetch", "copy"
        #[qproperty(QString, stage)]
        // Percentage of the restore, -1 when not known
        #[qproperty(i32, progress)]
        type RestoreManager = super::RestoreManagerRust;

        // The data backups in a folder <place>/astroarch/data-backup, JSON:
        // [{id, folder, snapshots (newest first), hasData, thisSystem}]
        #[qinvokable]
        fn systems(self: &RestoreManager, folder: &QString) -> QString;

        // Partitions that may hold an old AstroArch, JSON: [{device, label, fstype, size,
        // mountpoint, astroarch (its label says so), opened (mounted by the helper: root)}]
        #[qinvokable]
        fn disks(self: &RestoreManager) -> QString;

        // `source` (all calls): JSON {kind: "backup", folder, snapshot} | {kind: "disk",
        // root, name} | {kind: "astromonitor", root} | {kind: "system", root, name}
        // What it holds, JSON: {user, kiosk, version, hostname, found}
        #[qinvokable]
        fn describe(self: &RestoreManager, source: &QString) -> QString;

        // JSON {groups (SelectionList rows), excluded (the keys unticked at first)}
        #[qinvokable]
        fn rows(self: &RestoreManager, source: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "requestSizes"]
        fn request_sizes(self: Pin<&mut RestoreManager>, source: &QString);

        // Programs that must be closed first: "kstars,phd2.bin,indiserver" or ""
        #[qinvokable]
        #[cxx_name = "runningApps"]
        fn running_apps(self: &RestoreManager) -> QString;

        // The token of AstroMonitor on this system, or ""
        #[qinvokable]
        #[cxx_name = "astromonitorToken"]
        fn astromonitor_token(self: &RestoreManager) -> QString;

        // Downloads the AstroMonitor backup: finished("fetch", code, root)
        #[qinvokable]
        #[cxx_name = "fetchAstromonitor"]
        fn fetch_astromonitor(self: Pin<&mut RestoreManager>, token: &QString);

        // finished("mount", code, root)
        #[qinvokable]
        #[cxx_name = "mountDisk"]
        fn mount_disk(self: Pin<&mut RestoreManager>, device: &QString);

        // finished("unmount", code, "")
        #[qinvokable]
        #[cxx_name = "unmountDisk"]
        fn unmount_disk(self: Pin<&mut RestoreManager>);

        // `excluded`: JSON array of unticked keys. finished("restore", code, JSON {aside,
        // skipped, entries})
        #[qinvokable]
        fn restore(self: Pin<&mut RestoreManager>, source: &QString, excluded: &QString);

        // Earlier restores that can be undone, newest first, JSON:
        // [{stamp, time, label, folders, files}]
        #[qinvokable]
        fn asides(self: &RestoreManager) -> QString;

        // `folders`: JSON array of the folders of one restore. finished("putback", code, "")
        #[qinvokable]
        #[cxx_name = "putBack"]
        fn put_back(self: Pin<&mut RestoreManager>, folders: &QString);

        #[qsignal]
        #[cxx_name = "sizeReady"]
        fn size_ready(self: Pin<&mut RestoreManager>, key: &QString, bytes: f64);

        // `code`: "" done, "auth" (password refused or window closed), "busy" (detail: the
        // programs), "missing" (no helper), "notfound", "network", or the helper's code
        #[qsignal]
        fn finished(self: Pin<&mut RestoreManager>, action: &QString, code: &QString, detail: &QString);
    }

    impl cxx_qt::Threading for RestoreManager {}
}

#[derive(Default)]
pub struct RestoreManagerRust {
    busy: bool,
    stage: QString,
    progress: i32,
}

fn q(text: &str) -> QString {
    QString::from(text)
}

const INSTALLED_HELPER: &str = "/usr/lib/astroarch-control-center/astroarch-restore-helper";
/// Where the helper mounts an old disk
const MOUNTS: &str = "/run/astroarch-cc-restore";
/// File systems an AstroArch system can be on
const SYSTEM_FS: &[&str] = &["ext4", "ext3", "ext2", "btrfs", "xfs", "f2fs"];
/// Programs that write the profiles: a restore waits until they are closed
const WRITERS: &[&str] = &["kstars", "phd2.bin", "indiserver"];
const ASTROMONITOR_SERVER: &str = "http://astromatto.com:11111/backup/db/";
/// Where each file of the AstroMonitor archive goes (its src/backup/database.rs)
const ASTROMONITOR_FILES: &[(&str, &str)] = &[
    ("backup/indi", "~/.indi"),
    ("backup/kstars/userdb.sqlite", "~/.local/share/kstars/userdb.sqlite"),
    ("backup/kstars/mycitydb.sqlite", "~/.local/share/kstars/mycitydb.sqlite"),
    ("backup/kstars/fov.dat", "~/.local/share/kstars/fov.dat"),
    ("backup/kstars/kstarsrc", "~/.config/kstarsrc"),
    ("backup/.PHDGuidingV2", "~/.PHDGuidingV2"),
];

/// The helper: $ASTROARCH_RESTORE_HELPER, the installed path, else the one in this crate
/// (development build)
fn helper_path() -> Option<PathBuf> {
    [
        std::env::var_os("ASTROARCH_RESTORE_HELPER").map(PathBuf::from),
        Some(PathBuf::from(INSTALLED_HELPER)),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/astroarch-restore-helper.sh")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())).join(".cache"))
        .join("astroarch/restore-astromonitor")
}

// ------------------------------------------------------------------------ source

/// Files of another system at their absolute paths, under `settings` or `data`
#[derive(Debug, Clone)]
pub struct Source {
    pub settings: PathBuf,
    pub data: PathBuf,
    /// The homes on the system the files come from
    pub homes: Homes,
    /// "kind|detail", kept with what was set aside
    pub label: String,
}

impl Source {
    pub fn parse(spec: &str, here: &Homes) -> Result<Source, String> {
        let v: Value = serde_json::from_str(spec).map_err(|_| "source".to_string())?;
        let text = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let (settings, data, label) = match text("kind").as_str() {
            "backup" => {
                let folder = PathBuf::from(text("folder"));
                let snapshot = text("snapshot");
                if snapshot.contains('/') || snapshot.starts_with('.') {
                    return Err("snapshot".into());
                }
                let id = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                (folder.join("settings").join(&snapshot), folder.join("data"), format!("backup|{id} {snapshot}"))
            }
            kind @ ("disk" | "astromonitor" | "system") => {
                let root = PathBuf::from(text("root"));
                (root.clone(), root, format!("{kind}|{}", text("name")))
            }
            _ => return Err("source".into()),
        };
        if !settings.is_absolute() || !data.is_absolute() {
            return Err("source".into());
        }
        let homes = Source::find_homes(&[&settings, &data], here);
        Ok(Source { settings, data, homes, label })
    }

    /// The user of the old system: the same name as here when it is there, else the first
    /// home that is not a kiosk's; its kiosk user is <user>-kiosk
    fn find_homes(roots: &[&Path], here: &Homes) -> Homes {
        let names: BTreeSet<String> = roots
            .iter()
            .flat_map(|r| profile::subfolders(&r.join("home")))
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        let main_here = !here.user.ends_with("-kiosk");
        let user = if (names.contains(&here.user) && main_here) || names.is_empty() {
            here.user.clone()
        } else {
            names.iter().find(|n| !n.ends_with("-kiosk")).cloned().unwrap_or_else(|| here.user.clone())
        };
        let kiosk = format!("{user}-kiosk");
        Homes {
            home: if names.contains(&user) { PathBuf::from("/home").join(&user) } else { here.home.clone() },
            kiosk: names.contains(&kiosk).then(|| PathBuf::from("/home").join(&kiosk)),
            user,
        }
    }

    /// Where a path of the old system is in the source
    pub fn locate(&self, path: &Path, kind: Kind) -> PathBuf {
        let root = if kind == Kind::Settings { &self.settings } else { &self.data };
        root.join(path.strip_prefix("/").unwrap_or(path))
    }
}

/// The same path on this system: the old homes become the new ones; None for a kiosk path
/// when this system has no kiosk user
pub fn relocate(path: &Path, old: &Homes, new: &Homes) -> Option<PathBuf> {
    if let Some(k) = &old.kiosk {
        if let Ok(rest) = path.strip_prefix(k) {
            return new.kiosk.as_ref().map(|nk| nk.join(rest));
        }
    }
    match path.strip_prefix(&old.home) {
        Ok(rest) => Some(new.home.join(rest)),
        Err(_) => Some(path.to_path_buf()),
    }
}

/// A link on the way from `base` to `path` (included): the kiosk shares this with the
/// main user, so it is restored with the main user's files
fn through_link(path: &Path, base: &Path) -> bool {
    path.ancestors().take_while(|a| *a != base && a.starts_with(base)).any(|a| a.is_symlink())
}

// -------------------------------------------------------------------------- plan

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    pub owner: String,
    pub src: PathBuf,
    pub dst: PathBuf,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub entries: Vec<Entry>,
    /// Source paths never copied
    pub excludes: Vec<PathBuf>,
    /// Targets left alone: shared with the main user here
    pub shared: Vec<PathBuf>,
}

fn kiosk_user(here: &Homes) -> String {
    format!("{}-kiosk", here.user)
}

/// What to copy for the ticked rows (`excluded`: the keys unticked, as in the data backup)
pub fn plan(catalog: &Catalog, source: &Source, excluded: &[String], here: &Homes) -> Plan {
    let config = Config { excluded: excluded.to_vec(), ..Config::default() };
    let p = profile::plan(catalog, &config, &source.homes);
    let mut out = Plan::default();
    for ex in &p.excludes {
        for kind in [Kind::Settings, Kind::Data] {
            let s = source.locate(ex, kind);
            if !out.excludes.contains(&s) {
                out.excludes.push(s);
            }
        }
    }
    for (kind, list) in [(Kind::Settings, &p.settings), (Kind::Data, &p.data)] {
        for path in list {
            let src = source.locate(path, kind);
            let Some(dst) = relocate(path, &source.homes, here) else { continue };
            if !present(&src) {
                continue;
            }
            let (owner, base) = match &here.kiosk {
                Some(k) if dst.starts_with(k) => (kiosk_user(here), k.clone()),
                _ if dst.starts_with(&here.home) => (here.user.clone(), here.home.clone()),
                _ => (here.user.clone(), dst.clone()),
            };
            if through_link(&dst, &base) {
                out.shared.push(dst);
                out.excludes.push(src);
                continue;
            }
            out.entries.push(Entry { kind, owner, src, dst });
        }
    }
    // An entry inside another one is copied with it
    let all = out.entries.clone();
    out.entries.retain(|e| !all.iter().any(|o| o.kind == e.kind && o.src != e.src && e.src.starts_with(&o.src)));
    out
}

/// The helper's input (see scripts/astroarch-restore-helper.sh); paths with a tab or a
/// line break are left out
pub fn plan_text(plan: &Plan, source: &Source) -> String {
    let ok = |p: &Path| !p.to_string_lossy().contains(['\t', '\n']);
    let mut lines = vec![format!("label\t{}", source.label.replace(['\t', '\n'], " "))];
    for root in [&source.settings, &source.data] {
        if root.is_dir() && ok(root) {
            lines.push(format!("root\t{}", root.display()));
        }
    }
    lines.dedup();
    for ex in plan.excludes.iter().filter(|p| ok(p)) {
        lines.push(format!("exclude\t{}", ex.display()));
    }
    for e in plan.entries.iter().filter(|e| ok(&e.src) && ok(&e.dst)) {
        let kind = if e.kind == Kind::Settings { "settings" } else { "data" };
        lines.push(format!("{kind}\t{}\t{}\t{}", e.owner, e.src.display(), e.dst.display()));
    }
    lines.join("\n") + "\n"
}

/// Rows of the selection list read in the source, without what cannot go anywhere here
/// (the kiosk rows when this system has no kiosk user); the kiosk's profiles shared with
/// the main user here get a note
pub fn rows(catalog: &Catalog, source: &Source, here: &Homes) -> Value {
    let locate = |p: &Path, k: Kind| source.locate(p, k);
    let mut groups = profile::rows_in(catalog, &source.homes, &locate);
    let kiosk_here = |shown: &str| -> Option<PathBuf> {
        let old = source.homes.kiosk.as_ref()?;
        let rest = Path::new(shown).strip_prefix(old).ok()?;
        Some(here.kiosk.as_ref()?.join(rest))
    };
    if let Some(list) = groups.as_array_mut() {
        for g in list.iter_mut() {
            if let Some(items) = g["items"].as_array_mut() {
                items.retain(|it| {
                    let first = it["paths"][0].as_str().unwrap_or("");
                    !(source.homes.kiosk.as_ref().is_some_and(|k| Path::new(first).starts_with(k)) && here.kiosk.is_none())
                });
                for it in items.iter_mut() {
                    if let Some(children) = it["children"].as_array_mut() {
                        for c in children.iter_mut() {
                            let shared = c["paths"].as_array().is_some_and(|ps| {
                                !ps.is_empty()
                                    && ps.iter().all(|p| {
                                        kiosk_here(p.as_str().unwrap_or("")).is_some_and(|t| {
                                            through_link(&t, here.kiosk.as_deref().unwrap_or(Path::new("/")))
                                        })
                                    })
                            });
                            if shared {
                                c["note"] = json!("Shared with the main desktop here: restored with it.");
                            }
                        }
                    }
                }
            }
        }
        list.retain(|g| g["items"].as_array().is_some_and(|a| !a.is_empty()));
    }
    // Unticked at first: what the backup leaves out, the images (the big part), and what
    // this backup does not have (else it would keep the KStars database in, for the darks)
    let mut excluded = profile::default_excluded(catalog);
    for item in groups.as_array().into_iter().flatten().flat_map(|g| g["items"].as_array().cloned().unwrap_or_default()) {
        if item["exists"] == false {
            excluded.push(item["id"].as_str().unwrap_or("").to_string());
        }
        for c in item["children"].as_array().into_iter().flatten() {
            if c["exists"] == false {
                excluded.push(c["key"].as_str().unwrap_or("").to_string());
            }
        }
    }
    for item in &catalog.item {
        if item.kind == Kind::Data {
            excluded.push(item.id.clone());
        }
        for c in &item.child {
            if c.kind == Some(Kind::Data) {
                excluded.push(format!("{}/{}", item.id, c.id));
            }
        }
    }
    json!({ "groups": groups, "excluded": excluded })
}

/// Lays out the AstroMonitor archive extracted in `extracted` as a source under `root`
pub fn layout_astromonitor(extracted: &Path, root: &Path, here: &Homes) -> std::io::Result<usize> {
    let mut found = 0;
    for (from, to) in ASTROMONITOR_FILES {
        let src = extracted.join(from);
        if !present(&src) {
            continue;
        }
        let Some(target) = here.expand(to) else { continue };
        let dst = root.join(target.strip_prefix("/").unwrap_or(&target));
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&src, &dst)?;
        found += 1;
    }
    Ok(found)
}

/// Does the token look like one (it ends up in a URL)?
fn token_ok(token: &str) -> bool {
    !token.is_empty() && token.len() <= 200 && token.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

// ------------------------------------------------------------------------ system

fn running_writers() -> Vec<String> {
    let mut found = BTreeSet::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        if let Ok(comm) = std::fs::read_to_string(e.path().join("comm")) {
            let comm = comm.trim();
            if WRITERS.contains(&comm) {
                found.insert(comm.to_string());
            }
        }
    }
    found.into_iter().collect()
}

/// A mount point of its own (not the folder of the file system above)
fn is_mounted(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(path), path.parent().map(std::fs::metadata)) {
        (Ok(a), Some(Ok(b))) => a.dev() != b.dev(),
        _ => false,
    }
}

/// Runs the helper with pkexec, `input` on its stdin; `on_record` gets each line of its
/// output. Returns (code, detail) as `finished` says them
fn run_helper(args: &[&str], input: &str, mut on_record: impl FnMut(&str)) -> (String, String) {
    let Some(helper) = helper_path() else {
        return ("missing".into(), INSTALLED_HELPER.into());
    };
    let mut child = match Command::new("pkexec")
        .arg(&helper)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return ("failed".into(), format!("pkexec: {err}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let stderr = child.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = err.read_to_string(&mut text);
            text
        })
    });
    let mut error: Option<(String, String)> = None;
    let mut busy = Vec::new();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(rest) = line.strip_prefix("ERROR:") {
                let (code, detail) = rest.split_once(':').unwrap_or((rest, ""));
                error = Some((code.to_string(), detail.trim().to_string()));
            } else if let Some(app) = line.strip_prefix("BUSY:") {
                busy.push(app.to_string());
            }
            on_record(&line);
        }
    }
    let status = child.wait();
    let stderr = stderr.and_then(|t| t.join().ok()).unwrap_or_default();
    match (status.map(|s| s.code()), error) {
        (Ok(Some(0)), _) => (String::new(), String::new()),
        (Ok(Some(4)), _) => ("busy".into(), busy.join(",")),
        (_, Some(error)) => error,
        // pkexec: 126 = the password window was closed, 127 = not authorized
        (Ok(Some(126 | 127)), None) => ("auth".into(), String::new()),
        (_, None) => ("failed".into(), stderr.lines().last().unwrap_or("").to_string()),
    }
}

// ------------------------------------------------------------------------ QObject

impl qobject::RestoreManager {
    pub fn systems(&self, folder: &QString) -> QString {
        let base = PathBuf::from(folder.to_string());
        let this_id = profile::load_config().map(|c| c.id).unwrap_or_default();
        let list: Vec<Value> = profile::subfolders(&base)
            .into_iter()
            .filter_map(|dir| {
                let id = dir.file_name()?.to_string_lossy().to_string();
                let settings = dir.join("settings");
                let mut snapshots = profile::snapshots(&settings);
                // Syncthing: one copy, the computer keeps the versions
                if settings.join("current").is_dir() {
                    snapshots.push("current".into());
                }
                snapshots.reverse();
                let has_data = std::fs::read_dir(dir.join("data")).is_ok_and(|mut d| d.next().is_some());
                if snapshots.is_empty() && !has_data {
                    return None;
                }
                Some(json!({ "id": id, "folder": dir, "snapshots": snapshots, "hasData": has_data,
                             "thisSystem": id == this_id }))
            })
            .collect();
        q(&Value::Array(list).to_string())
    }

    pub fn disks(&self) -> QString {
        let list: Vec<Value> = media::list(0)
            .into_iter()
            .filter(|d| SYSTEM_FS.contains(&d["fstype"].as_str().unwrap_or("")))
            .map(|d| {
                let device = d["device"].as_str().unwrap_or("").to_string();
                let name = device.rsplit('/').next().unwrap_or("").to_string();
                let root = Path::new(MOUNTS).join(&name);
                let label = d["label"].as_str().unwrap_or("");
                json!({ "device": device, "label": label, "fstype": d["fstype"], "size": d["size"],
                        "mountpoint": d["mountpoint"], "astroarch": label.starts_with("ALARM_"),
                        "opened": if is_mounted(&root) { root.to_string_lossy().to_string() } else { String::new() } })
            })
            .collect();
        q(&Value::Array(list).to_string())
    }

    pub fn describe(&self, source: &QString) -> QString {
        let here = Homes::current();
        let Ok(s) = Source::parse(&source.to_string(), &here) else { return q("{}") };
        let read = |p: PathBuf| std::fs::read_to_string(p).map(|t| t.trim().to_string()).unwrap_or_default();
        let home = s.locate(&s.homes.home, Kind::Settings);
        q(&json!({
            "user": s.homes.user,
            "kiosk": s.homes.kiosk.is_some(),
            "found": home.is_dir(),
            "version": read(home.join(".astroarch/configs/.astroarch.version")),
            "hostname": read(s.settings.join("etc/hostname")),
        })
        .to_string())
    }

    pub fn rows(&self, source: &QString) -> QString {
        let here = Homes::current();
        match Source::parse(&source.to_string(), &here) {
            Ok(s) => q(&rows(&profile::catalog(), &s, &here).to_string()),
            Err(_) => q(r#"{"groups":[],"excluded":[]}"#),
        }
    }

    pub fn request_sizes(self: Pin<&mut Self>, source: &QString) {
        let here = Homes::current();
        let Ok(s) = Source::parse(&source.to_string(), &here) else { return };
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let locate = |p: &Path, k: Kind| s.locate(p, k);
            for (key, paths) in profile::size_keys_in(&profile::catalog(), &s.homes, &locate) {
                // Not readable by this user (an old disk's other homes): no size, not 0
                if paths.iter().all(|p| std::fs::symlink_metadata(p).is_err()) {
                    continue;
                }
                let bytes = profile::size_of(&paths) as f64;
                let _ = qt_thread.queue(move |mut o| o.as_mut().size_ready(&q(&key), bytes));
            }
        });
    }

    pub fn running_apps(&self) -> QString {
        q(&running_writers().join(","))
    }

    pub fn astromonitor_token(&self) -> QString {
        let path = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/astromonitor/astro.json");
        let token = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["token"].as_str().map(String::from))
            .unwrap_or_default();
        q(&token)
    }

    pub fn fetch_astromonitor(mut self: Pin<&mut Self>, token: &QString) {
        let token = token.to_string().trim().to_string();
        if *self.busy() {
            return;
        }
        if !token_ok(&token) {
            self.as_mut().finished(&q("fetch"), &q("token"), &q(""));
            return;
        }
        self.as_mut().set_busy(true);
        self.as_mut().set_stage(q("fetch"));
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let (code, detail) = match fetch(&token, &Homes::current()) {
                Ok(root) => (String::new(), root.to_string_lossy().to_string()),
                Err((code, detail)) => (code, detail),
            };
            let _ = qt_thread.queue(move |mut o| {
                o.as_mut().set_busy(false);
                o.as_mut().set_stage(q(""));
                o.as_mut().finished(&q("fetch"), &q(&code), &q(&detail));
            });
        });
    }

    pub fn mount_disk(mut self: Pin<&mut Self>, device: &QString) {
        let device = device.to_string();
        let valid = device.starts_with("/dev/")
            && device.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'));
        if *self.busy() || !valid {
            return;
        }
        self.as_mut().run_in_background("mount", vec!["mount".into(), device], String::new());
    }

    pub fn unmount_disk(mut self: Pin<&mut Self>) {
        if *self.busy() {
            return;
        }
        self.as_mut().run_in_background("unmount", vec!["unmount".into()], String::new());
    }

    pub fn restore(mut self: Pin<&mut Self>, source: &QString, excluded: &QString) {
        if *self.busy() {
            return;
        }
        let running = running_writers();
        if !running.is_empty() {
            self.as_mut().finished(&q("restore"), &q("busy"), &q(&running.join(",")));
            return;
        }
        let here = Homes::current();
        let Ok(s) = Source::parse(&source.to_string(), &here) else {
            self.as_mut().finished(&q("restore"), &q("source"), &q(""));
            return;
        };
        let excluded: Vec<String> = serde_json::from_str(&excluded.to_string()).unwrap_or_default();
        let plan = plan(&profile::catalog(), &s, &excluded, &here);
        if plan.entries.is_empty() {
            self.as_mut().finished(&q("restore"), &q("empty"), &q(""));
            return;
        }
        let input = plan_text(&plan, &s);
        self.as_mut().run_in_background("restore", vec!["restore".into()], input);
    }

    pub fn asides(&self) -> QString {
        let here = Homes::current();
        let mut by_stamp: std::collections::BTreeMap<String, Value> = Default::default();
        for home in std::iter::once(&here.home).chain(here.kiosk.as_ref()) {
            for dir in profile::subfolders(&home.join(".local/share/astroarch")) {
                let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let Some(stamp) = name.strip_prefix("restore-") else { continue };
                let info = std::fs::read_to_string(dir.join("info")).unwrap_or_default();
                let mut lines = info.lines();
                let label = lines.next().unwrap_or("").to_string();
                let time: u64 = lines.next().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
                let files = std::fs::read(dir.join("restored.list")).map(|b| b.iter().filter(|c| **c == 0).count()).unwrap_or(0);
                let entry = by_stamp
                    .entry(stamp.to_string())
                    .or_insert_with(|| json!({ "stamp": stamp, "time": time, "label": label, "folders": [], "files": 0 }));
                entry["folders"].as_array_mut().map(|a| a.push(json!(dir)));
                entry["files"] = json!(entry["files"].as_u64().unwrap_or(0) + files as u64);
            }
        }
        q(&Value::Array(by_stamp.into_values().rev().collect()).to_string())
    }

    pub fn put_back(mut self: Pin<&mut Self>, folders: &QString) {
        if *self.busy() {
            return;
        }
        let running = running_writers();
        if !running.is_empty() {
            self.as_mut().finished(&q("putback"), &q("busy"), &q(&running.join(",")));
            return;
        }
        let folders: Vec<String> = serde_json::from_str(&folders.to_string()).unwrap_or_default();
        if folders.is_empty() {
            return;
        }
        let mut args = vec!["undo".to_string()];
        args.extend(folders);
        self.as_mut().run_in_background("putback", args, String::new());
    }

    /// The helper in a thread: stage and progress follow its output, then `finished`
    fn run_in_background(mut self: Pin<&mut Self>, action: &'static str, args: Vec<String>, input: String) {
        self.as_mut().set_busy(true);
        self.as_mut().set_stage(q("auth"));
        self.as_mut().set_progress(-1);
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let records = qt_thread.clone();
            let mut result = String::new();
            let mut aside = Vec::new();
            let mut skipped = Vec::new();
            let mut entries = 0;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (code, detail) = run_helper(&refs, &input, |line| {
                if let Some(r) = line.strip_prefix("RESULT:") {
                    result = r.to_string();
                } else if let Some(a) = line.strip_prefix("ASIDE:") {
                    aside.push(a.to_string());
                } else if let Some(s) = line.strip_prefix("SKIPPED:") {
                    skipped.push(s.to_string());
                }
                let line = line.to_string();
                if let Some((i, n)) = line.strip_prefix("PROGRESS:").and_then(|p| p.split_once(':')) {
                    let (i, n) = (i.parse::<i32>().unwrap_or(0), n.parse::<i32>().unwrap_or(1).max(1));
                    entries = n;
                    let _ = records.queue(move |mut o| {
                        o.as_mut().set_stage(q("copy"));
                        o.as_mut().set_progress(((i - 1) * 100 / n).clamp(0, 100));
                    });
                } else if line.starts_with("STAGE:") || line.starts_with("RESULT:") {
                    let _ = records.queue(move |mut o| o.as_mut().set_stage(q(action)));
                }
            });
            let detail = if code.is_empty() {
                match action {
                    "restore" => json!({ "aside": aside, "skipped": skipped, "entries": entries }).to_string(),
                    _ => result,
                }
            } else {
                detail
            };
            let _ = qt_thread.queue(move |mut o| {
                o.as_mut().set_busy(false);
                o.as_mut().set_stage(q(""));
                o.as_mut().set_progress(-1);
                o.as_mut().finished(&q(action), &q(&code), &q(&detail));
            });
        });
    }
}

/// Downloads and lays out the AstroMonitor backup (as its "Restore Backup" reads it:
/// GET <server>/backup/db/<token>, a tar archive). Err: (code, detail)
fn fetch(token: &str, here: &Homes) -> Result<PathBuf, (String, String)> {
    let base = cache_dir();
    let _ = std::fs::remove_dir_all(&base);
    let extracted = base.join("extracted");
    let root = base.join("root");
    let io = |e: std::io::Error| ("write".to_string(), e.to_string());
    std::fs::create_dir_all(&extracted).map_err(io)?;
    std::fs::create_dir_all(&root).map_err(io)?;
    let archive = base.join("backup.tar");
    let out = Command::new("curl")
        .args(["-sS", "-m", "120", "-w", "%{http_code}", "-o"])
        .arg(&archive)
        .arg(format!("{ASTROMONITOR_SERVER}{token}"))
        .output()
        .map_err(|e| ("network".to_string(), e.to_string()))?;
    let http = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        return Err(("network".into(), String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    match http.as_str() {
        "200" => {}
        "404" => return Err(("notfound".into(), String::new())),
        other => return Err(("network".into(), format!("HTTP {other}"))),
    }
    let out = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&extracted)
        .args(["--no-same-owner", "--no-same-permissions"])
        .output()
        .map_err(|e| ("archive".to_string(), e.to_string()))?;
    if !out.status.success() {
        return Err(("archive".into(), String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    match layout_astromonitor(&extracted, &root, here) {
        Ok(0) => Err(("notfound".into(), String::new())),
        Ok(_) => {
            let _ = std::fs::remove_file(&archive);
            Ok(root)
        }
        Err(e) => Err(io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aa-restore-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn here(base: &Path, kiosk: bool) -> Homes {
        Homes {
            user: "new".into(),
            home: base.join("home/new"),
            kiosk: kiosk.then(|| base.join("home/new-kiosk")),
        }
    }

    /// An old disk of user "astronaut" with a kiosk, images and a link of shared profiles
    fn old_disk(root: &Path) {
        file(&root.join("home/astronaut/.indi/EQMod Mount_config.xml"), "mount");
        file(&root.join("home/astronaut/.local/share/kstars/userdb.sqlite"), "db");
        file(&root.join("home/astronaut/.config/kstarsrc"), "rc");
        file(&root.join("home/astronaut/.PHDGuidingV2"), "phd");
        file(&root.join("home/astronaut-kiosk/.indi/cam.xml"), "kiosk cam");
        file(&root.join("home/astronaut-kiosk/.config/kwinrc"), "kwin");
        file(&root.join("srv/astro-data/Light/M31/a.fits"), "img");
        std::os::unix::fs::symlink("/home/astronaut/.phd2", root.join("home/astronaut-kiosk/.phd2")).unwrap();
    }

    fn source(root: &Path, here: &Homes) -> Source {
        Source::parse(&json!({ "kind": "disk", "root": root }).to_string(), here).unwrap()
    }

    #[test]
    fn homes_of_the_old_system() {
        let base = tmp("homes");
        let root = base.join("disk");
        old_disk(&root);
        let s = source(&root, &here(&base, true));
        assert_eq!(s.homes.user, "astronaut");
        assert_eq!(s.homes.home, PathBuf::from("/home/astronaut"));
        assert_eq!(s.homes.kiosk, Some(PathBuf::from("/home/astronaut-kiosk")));
        // In the kiosk session: the old main user still, not the old kiosk
        let kiosk_here = Homes { user: "astronaut-kiosk".into(), home: base.join("k"), kiosk: None };
        assert_eq!(source(&root, &kiosk_here).homes.user, "astronaut");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn plan_maps_old_homes_to_new_ones() {
        let base = tmp("plan");
        let root = base.join("disk");
        old_disk(&root);
        let h = here(&base, true);
        let s = source(&root, &h);
        let p = plan(&profile::catalog(), &s, &[], &h);
        let find = |dst: PathBuf| p.entries.iter().find(|e| e.dst == dst).cloned();
        let indi = find(h.home.join(".indi")).expect("indi");
        assert_eq!(indi.owner, "new");
        assert_eq!(indi.src, root.join("home/astronaut/.indi"));
        assert_eq!(indi.kind, Kind::Settings);
        // The kiosk's rows are the kiosk user's, in its home
        let kiosk = find(base.join("home/new-kiosk/.indi")).expect("kiosk indi");
        assert_eq!(kiosk.owner, "new-kiosk");
        assert_eq!(kiosk.src, root.join("home/astronaut-kiosk/.indi"));
        assert!(find(base.join("home/new-kiosk/.config")).is_some());
        let images = find(PathBuf::from("/srv/astro-data")).expect("images");
        assert_eq!(images.kind, Kind::Data);
        // Missing in the source: nothing to copy
        assert!(find(h.home.join(".phd2")).is_none());
        let text = plan_text(&p, &s);
        assert!(text.starts_with("label\tdisk|\nroot\t"));
        assert!(text.contains(&format!("settings\tnew\t{}\t{}\n", root.join("home/astronaut/.indi").display(), h.home.join(".indi").display())));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn unticked_rows_and_no_kiosk_here() {
        let base = tmp("unticked");
        let root = base.join("disk");
        old_disk(&root);
        let h = here(&base, false);
        let s = source(&root, &h);
        let p = plan(&profile::catalog(), &s, &["indi".into(), "astro-data".into()], &h);
        assert!(p.entries.iter().all(|e| !e.dst.ends_with(".indi") && !e.dst.starts_with("/srv")));
        assert!(p.entries.iter().all(|e| e.owner == "new"));
        assert!(p.excludes.contains(&root.join("home/astronaut/.indi")));
        // No kiosk here: no kiosk rows
        let r = rows(&profile::catalog(), &s, &h);
        assert!(r["groups"].as_array().unwrap().iter().all(|g| g["id"] != "kiosk"));
        assert!(r["excluded"].as_array().unwrap().contains(&json!("astro-data")));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn shared_kiosk_profiles_are_left_alone() {
        let base = tmp("shared");
        let root = base.join("disk");
        old_disk(&root);
        let h = here(&base, true);
        std::fs::create_dir_all(h.home.join(".indi")).unwrap();
        std::fs::create_dir_all(h.kiosk.as_ref().unwrap()).unwrap();
        std::os::unix::fs::symlink(h.home.join(".indi"), h.kiosk.as_ref().unwrap().join(".indi")).unwrap();
        let s = source(&root, &h);
        // Only the kiosk's INDI row: its target is a link to the main user's
        let p = plan(&profile::catalog(), &s, &["kiosk/apps".into()], &h);
        assert!(p.shared.contains(&h.kiosk.as_ref().unwrap().join(".indi")));
        assert!(p.excludes.contains(&root.join("home/astronaut-kiosk/.indi")));
        assert!(!p.entries.iter().any(|e| e.owner == "new-kiosk" && e.dst.ends_with(".indi")));
        let r = rows(&profile::catalog(), &s, &h).to_string();
        assert!(r.contains("Shared with the main desktop here"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn backup_snapshot_and_data_roots() {
        let base = tmp("backup");
        let folder = base.join("data-backup/astroarch-20261029-abcd");
        let snap = folder.join("settings/snapshot-20261029-220554");
        file(&snap.join("home/astronaut/.indi/a.xml"), "a");
        file(&folder.join("data/srv/astro-data/Dark/d.fits"), "d");
        let h = here(&base, false);
        let spec = json!({ "kind": "backup", "folder": folder, "snapshot": "snapshot-20261029-220554" }).to_string();
        let s = Source::parse(&spec, &h).unwrap();
        assert_eq!(s.homes.home, PathBuf::from("/home/astronaut"));
        assert_eq!(s.locate(Path::new("/srv/astro-data"), Kind::Data), folder.join("data/srv/astro-data"));
        let r = rows(&profile::catalog(), &s, &h);
        let images = r["groups"].as_array().unwrap().iter().find(|g| g["id"] == "images").unwrap();
        assert_eq!(images["items"][0]["children"][0]["key"], "/srv/astro-data/Dark");
        assert!(Source::parse(&json!({ "kind": "backup", "folder": folder, "snapshot": "../x" }).to_string(), &h).is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn system_backup_snapshot_root() {
        let base = tmp("system");
        let root = base.join("browse/snapshots/2026-10-29_220554");
        old_disk(&root);
        file(&root.join("usr/bin/bash"), "elf");
        let h = here(&base, true);
        let spec = json!({ "kind": "system", "root": root, "name": "2026-10-29" }).to_string();
        let s = Source::parse(&spec, &h).unwrap();
        assert_eq!(s.settings, root);
        assert_eq!(s.data, root);
        assert_eq!(s.label, "system|2026-10-29");
        assert_eq!(s.homes.user, "astronaut");
        // Only the catalog's paths: never the system's own files
        let p = plan(&profile::catalog(), &s, &[], &h);
        assert!(p.entries.iter().any(|e| e.src == root.join("home/astronaut/.indi")));
        assert!(p.entries.iter().all(|e| e.src.starts_with(root.join("home")) || e.src.starts_with(root.join("srv/astro-data"))));
        assert!(Source::parse(&json!({ "kind": "system", "root": "relative" }).to_string(), &h).is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    /// The helper's input for a real source, to test the helper by hand on other users:
    /// RESTORE_SOURCE='{"kind":"disk","root":"/run/astroarch-cc-restore/loop0p2"}' RESTORE_USER=u
    /// RESTORE_HOME=/home/u RESTORE_KIOSK=/home/u-kiosk RESTORE_EXCLUDED='["astro-data"]'
    /// cargo test -p astroarch-control-center print_plan -- --ignored --nocapture
    #[test]
    #[ignore]
    fn print_plan() {
        let var = |k: &str| std::env::var(k).unwrap_or_default();
        let here = Homes {
            user: var("RESTORE_USER"),
            home: PathBuf::from(var("RESTORE_HOME")),
            kiosk: Some(var("RESTORE_KIOSK")).filter(|k| !k.is_empty()).map(PathBuf::from),
        };
        let s = Source::parse(&var("RESTORE_SOURCE"), &here).unwrap();
        let excluded: Vec<String> = serde_json::from_str(&var("RESTORE_EXCLUDED")).unwrap_or_default();
        let p = plan(&profile::catalog(), &s, &excluded, &here);
        eprintln!("shared: {:?}", p.shared);
        print!("{}", plan_text(&p, &s));
    }

    /// Asks the AstroMonitor server (read only) for a token that does not exist
    #[test]
    #[ignore]
    fn astromonitor_unknown_token() {
        let base = tmp("unknown-token");
        let h = here(&base, false);
        std::env::set_var("XDG_CACHE_HOME", base.join("cache"));
        assert_eq!(fetch("not-a-real-token-0000", &h).unwrap_err().0, "notfound");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn astromonitor_archive_layout() {
        let base = tmp("astromonitor");
        let ex = base.join("extracted");
        file(&ex.join("backup/indi/EQMod Mount_config.xml"), "m");
        file(&ex.join("backup/kstars/userdb.sqlite"), "db");
        file(&ex.join("backup/kstars/kstarsrc"), "rc");
        file(&ex.join("backup/.PHDGuidingV2"), "phd");
        let h = here(&base, false);
        let root = base.join("root");
        assert_eq!(layout_astromonitor(&ex, &root, &h).unwrap(), 4);
        let s = Source::parse(&json!({ "kind": "astromonitor", "root": root }).to_string(), &h).unwrap();
        assert_eq!(s.homes.home, h.home);
        let p = plan(&profile::catalog(), &s, &[], &h);
        let dsts: Vec<_> = p.entries.iter().map(|e| e.dst.clone()).collect();
        assert!(dsts.contains(&h.home.join(".indi")));
        assert!(dsts.contains(&h.home.join(".local/share/kstars/userdb.sqlite")));
        assert!(dsts.contains(&h.home.join(".config/kstarsrc")));
        assert!(token_ok("123456:AbC-d_e") && !token_ok("a/b") && !token_ok(""));
        let _ = std::fs::remove_dir_all(base);
    }
}

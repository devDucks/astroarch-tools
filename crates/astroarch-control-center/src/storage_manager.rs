//! QObject of the Image storage page (qml/StorageView.qml): lists the drives and runs
//! the root helper that moves the shared folder /srv/astro-data to a drive and back
//! (scripts/astroarch-storage-helper.sh, started with pkexec).
//!
//! The drives come from the backup tool's code (media.rs and storage.rs of
//! astroarch-backup, compiled in by main.rs), without another system's disk (drives.rs):
//! the same list as the Data backup page.

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{drives, storage};

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
        // Where the helper is: "auth" (waiting for the password), "prepare", "copy", "switch"
        #[qproperty(QString, stage)]
        // Percentage of the copy, -1 when not known
        #[qproperty(i32, progress)]
        type StorageManager = super::StorageManagerRust;

        /// Reads the drives (asynchronously): drivesListed
        #[qinvokable]
        fn list_drives(self: Pin<&mut StorageManager>);

        /// Runs the helper (asynchronously): "move" (to `device`), "back", "reconnect",
        /// "forget". Ends with finished
        #[qinvokable]
        fn run(self: Pin<&mut StorageManager>, action: &QString, device: &QString);

        /// JSON array: device, label, fstype, size and free (bytes, free -1 when the drive
        /// is not mounted), mountpoint, state ("ok", "fat", "readonly", "unsupported"),
        /// hasData (an astroarch/astro-data folder already there; null when not known)
        #[qsignal]
        #[cxx_name = "drivesListed"]
        fn drives_listed(self: Pin<&mut StorageManager>, json: QString);

        /// `code`: "" when done, "auth" (password refused or window closed), "missing"
        /// (no helper), else the helper's error code; `detail`: its message
        #[qsignal]
        fn finished(self: Pin<&mut StorageManager>, action: QString, code: QString, detail: QString);
    }

    impl cxx_qt::Threading for StorageManager {}
}

#[derive(Default)]
pub struct StorageManagerRust {
    busy: bool,
    stage: QString,
    progress: i32,
}

const INSTALLED_HELPER: &str = "/usr/lib/astroarch-control-center/astroarch-storage-helper";

/// File systems the helper can use (the same lists as in the helper)
const USABLE_FS: &[&str] = &["ext4", "ext3", "ext2", "btrfs", "xfs", "f2fs", "exfat", "vfat", "ntfs"];

/// The helper: $ASTROARCH_STORAGE_HELPER, the installed path, else the copy in this crate
/// (development build)
fn helper_path() -> Option<PathBuf> {
    [
        std::env::var_os("ASTROARCH_STORAGE_HELPER").map(PathBuf::from),
        Some(PathBuf::from(INSTALLED_HELPER)),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/astroarch-storage-helper.sh")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

/// The drives of the backup tool, with what the image storage can do with each
fn drives() -> Vec<Value> {
    drives::list(0)
        .into_iter()
        .map(|d| {
            let text = |key: &str| d[key].as_str().unwrap_or("").to_string();
            let (fstype, label, mountpoint) = (text("fstype"), text("label"), text("mountpoint"));
            let free = if mountpoint.is_empty() {
                -1
            } else {
                storage::available_space(Path::new(&mountpoint)).map_or(-1, |b| b as i64)
            };
            let state = if d["state"] == "readonly" {
                "readonly"
            } else if !USABLE_FS.contains(&fstype.as_str()) {
                "unsupported"
            } else if fstype == "vfat" {
                "fat"
            } else {
                "ok"
            };
            // An AstroArch folder already on the drive (the files are merged into it);
            // null when the drive is not mounted, so not known
            let has_data = if mountpoint.is_empty() {
                Value::Null
            } else {
                Path::new(&mountpoint).join("astroarch/astro-data").is_dir().into()
            };
            json!({
                "device": d["device"], "label": label, "fstype": fstype, "size": d["size"],
                "free": free, "mountpoint": mountpoint, "state": state, "hasData": has_data,
            })
        })
        .collect()
}

/// Runs the helper with pkexec; `on_record` gets each line of its output.
/// Returns (code, detail) as in `finished`
fn run_helper(action: &str, device: &str, mut on_record: impl FnMut(&str)) -> (String, String) {
    let Some(helper) = helper_path() else {
        return ("missing".into(), INSTALLED_HELPER.into());
    };
    let mut command = Command::new("pkexec");
    command.arg(&helper).arg(action);
    if action == "move" {
        command.arg(device);
    }
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return ("failed".into(), format!("pkexec: {err}")),
    };

    // stderr in another thread, so that neither pipe can fill up and block the helper
    let stderr = child.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = err.read_to_string(&mut text);
            text
        })
    });
    let mut error: Option<(String, String)> = None;
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(rest) = line.strip_prefix("ERROR:") {
                let (code, detail) = rest.split_once(':').unwrap_or((rest, ""));
                error = Some((code.to_string(), detail.trim().to_string()));
            }
            on_record(&line);
        }
    }
    let status = child.wait();
    let stderr = stderr.and_then(|t| t.join().ok()).unwrap_or_default();

    match (status.map(|s| s.code()), error) {
        (Ok(Some(0)), _) => (String::new(), String::new()),
        (_, Some(error)) => error,
        // pkexec: 126 = the password window was closed, 127 = not authorized
        (Ok(Some(126 | 127)), None) => ("auth".into(), String::new()),
        (_, None) => ("failed".into(), stderr.lines().last().unwrap_or("").to_string()),
    }
}

impl qobject::StorageManager {
    pub fn list_drives(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let json = Value::Array(drives()).to_string();
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().drives_listed(QString::from(json.as_str()));
            });
        });
    }

    pub fn run(mut self: Pin<&mut Self>, action: &QString, device: &QString) {
        let (action, device) = (action.to_string(), device.to_string());
        if *self.busy() || !["move", "back", "reconnect", "forget"].contains(&action.as_str()) {
            return;
        }
        self.as_mut().set_busy(true);
        self.as_mut().set_stage(QString::from("auth"));
        self.as_mut().set_progress(-1);

        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let progress_thread = qt_thread.clone();
            let (code, detail) = run_helper(&action, &device, |record| {
                let record = record.to_string();
                let _ = progress_thread.queue(move |mut qobject| {
                    if let Some(stage) = record.strip_prefix("STAGE:") {
                        qobject.as_mut().set_stage(QString::from(stage));
                        qobject.as_mut().set_progress(-1);
                    } else if let Some(Ok(percent)) = record.strip_prefix("PROGRESS:").map(str::parse::<i32>) {
                        qobject.as_mut().set_progress(percent.clamp(0, 100));
                    }
                });
            });
            let _ = qt_thread.queue(move |mut qobject| {
                qobject.as_mut().set_busy(false);
                qobject.as_mut().set_stage(QString::from(""));
                qobject.as_mut().finished(
                    QString::from(action.as_str()),
                    QString::from(code.as_str()),
                    QString::from(detail.as_str()),
                );
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_is_found_in_a_development_build() {
        assert!(helper_path().is_some_and(|p| p.is_file()));
    }
}

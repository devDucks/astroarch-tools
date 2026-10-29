//! CXX-Qt bridge: exposes the backup logic to QML under the name
//! `BackupManager`.
//!
//! Backups are stored in a preallocated Btrfs image file, managed
//! by the root helper `scripts/astroarch-backup-helper.sh` (launched via pkexec).
//! This bridge performs NO privileged operations: it lists directories,
//! measures data, evaluates locations (mandatory primary disk:
//! /var/backups, or external media), launches the helper, reads its progress, and
//! relays cancellation.

use crate::config::ConfigManager;
use crate::disk_analysis::DiskAnalyzer;
use crate::media;
use crate::storage;

use core::pin::Pin;
use cxx_qt::Threading;
use serde_json::{json, Value};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]

        // Explicit QML names (camelCase): no reliance on an
        // automatic conversion from snake_case to camelCase.
        #[qproperty(QString, backup_status, cxx_name = "backupStatus")]
        #[qproperty(i32, progress_percent, cxx_name = "progressPercent")]
        #[qproperty(QString, current_file, cxx_name = "currentFile")]
        #[qproperty(bool, is_running, cxx_name = "isRunning")]

        type BackupManager = super::BackupManagerRust;

        #[qsignal]
        fn backupFinished(self: Pin<&mut BackupManager>, success: bool, error_message: &QString);

        #[qsignal]
        fn backupCanceled(self: Pin<&mut BackupManager>);

        // End of data acquisition (bytes = 0 and error is not empty if the operation fails)
        #[qsignal]
        fn dataMeasured(self: Pin<&mut BackupManager>, bytes: i64, error: &QString);

        // Size of a folder in the selection window (absolute path)
        #[qsignal]
        fn folderSizeReady(self: Pin<&mut BackupManager>, path: &QString, bytes: i64);

        // Result of mounting a media file (empty mountpoint and non-empty error message if the operation fails)
        #[qsignal]
        fn mediaMounted(self: Pin<&mut BackupManager>, device: &QString, mountpoint: &QString, error: &QString);

        // keep: retention; excluded: most recently excluded files, one absolute path per line
        #[qsignal]
        fn configLoaded(self: Pin<&mut BackupManager>, keep: i32, excluded: &QString);

        // List of snapshots for a backup (JSON [{“name”,“boot”}]); returns an empty error if successful
        #[qsignal]
        fn snapshotsListed(self: Pin<&mut BackupManager>, json: &QString, error: &QString);

        // La restauration est lancée : le bureau va se fermer et le Raspberry Pi redémarrer seul
        #[qsignal]
        fn restoreStarted(self: Pin<&mut BackupManager>);

        // The restore did NOT start (nothing was changed)
        #[qsignal]
        fn restoreFailed(self: Pin<&mut BackupManager>, error: &QString);

        #[qinvokable]
        fn load_config(self: Pin<&mut BackupManager>);

        #[qinvokable]
        fn home_path(self: Pin<&mut BackupManager>) -> QString;

        // “/srv/astro-data” if it exists, otherwise “”
        #[qinvokable]
        fn shared_data_path(self: Pin<&mut BackupManager>) -> QString;

        // JSON: [{“name”,‘hasChildren’,“excludable”}]; path = ABSOLUTE path (personal
        // folder or shared folder, and their subfolders)
        #[qinvokable]
        fn list_subfolders(self: Pin<&mut BackupManager>, path: &QString, show_hidden: bool) -> QString;

        // Sizes of the subfolders in path, sent one by one (folderSizeReady signal)
        #[qinvokable]
        fn request_folder_sizes(self: Pin<&mut BackupManager>, path: &QString, show_hidden: bool);

        // Size of a single folder (folderSizeReady signal)
        #[qinvokable]
        fn request_path_size(self: Pin<&mut BackupManager>, path: &QString);

        // excluded_folders: absolute paths, separated by line breaks
        #[qinvokable]
        fn measure_data(self: Pin<&mut BackupManager>, excluded_folders: &QString);

        // JSON : {"problem","needed","available","existing","enough"}
        #[qinvokable]
        fn evaluate_destination(self: Pin<&mut BackupManager>, path: &QString) -> QString;

        // JSON: External media (not on the system disk) and their status
        #[qinvokable]
        fn list_media(self: Pin<&mut BackupManager>) -> QString;

        #[qinvokable]
        fn mount_media(self: Pin<&mut BackupManager>, device: &QString);

        // persist = false: backup before a restore (the usual retention
        // and exclusions remain unchanged)
        #[qinvokable]
        fn start_backup(
            self: Pin<&mut BackupManager>,
            destination: &QString,
            excluded_folders: &QString,
            keep: i32,
            persist: bool,
        );

        #[qinvokable]
        fn cancel_backup(self: Pin<&mut BackupManager>);

        // Is there an AstroArch backup in this destination folder?
        #[qinvokable]
        fn has_backup(self: Pin<&mut BackupManager>, path: &QString) -> bool;

        // Retrieves the list of snapshots (signal snapshotsListed)
        #[qinvokable]
        fn list_snapshots(self: Pin<&mut BackupManager>, destination: &QString);

        // FULL recovery of the system while it is running (restoreStarted / restoreFailed signals)
        #[qinvokable]
        fn start_restore(self: Pin<&mut BackupManager>, destination: &QString, snapshot: &QString);
    }

    impl cxx_qt::Threading for BackupManager {}
}

#[derive(Default)]
pub struct BackupManagerRust {
    pub backup_status: cxx_qt_lib::QString,
    pub progress_percent: i32,
    /// Code for the current step (preparing, copying, snapshot, verifying,
    /// shrinking, finishing): QML translates it
    pub current_file: cxx_qt_lib::QString,
    pub is_running: bool,
    backup_canceled: Arc<AtomicBool>,
    /// Bytes to back up, as measured by the last successful scan
    data_size: Arc<AtomicU64>,
}

type BackupThread = cxx_qt::CxxQtThread<qobject::BackupManager>;

enum Outcome {
    Success,
    Canceled,
    Failed(String),
}

impl qobject::BackupManager {
    pub fn load_config(self: Pin<&mut Self>) {
        let config = ConfigManager::load().unwrap_or_default();
        let keep = config.keep_clamped() as i32;
        let excluded = drop_protected(config.excluded_paths.clone()).join("\n");
        eprintln!("Config loaded: keep={}, {} excluded folder(s)", keep, config.excluded_paths.len());
        let excluded_q = cxx_qt_lib::QString::from(&excluded);
        self.configLoaded(keep, &excluded_q);
    }

    pub fn home_path(self: Pin<&mut Self>) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&home_dir().to_string_lossy().to_string())
    }

    pub fn shared_data_path(self: Pin<&mut Self>) -> cxx_qt_lib::QString {
        if Path::new(storage::SHARED_DATA_DIR).is_dir() {
            eprintln!("Shared data folder found: {}", storage::SHARED_DATA_DIR);
            cxx_qt_lib::QString::from(storage::SHARED_DATA_DIR)
        } else {
            eprintln!("Shared data folder not found: {}", storage::SHARED_DATA_DIR);
            cxx_qt_lib::QString::from("")
        }
    }

    /// Subfolders (actual folders, not symbolic links) of `path` (absolute)
    pub fn list_subfolders(
        self: Pin<&mut Self>,
        path: &cxx_qt_lib::QString,
        show_hidden: bool,
    ) -> cxx_qt_lib::QString {
        let parent = path.to_string();
        let home = home_dir().to_string_lossy().to_string();
        let items: Vec<Value> = list_dirs(&parent, show_hidden)
            .into_iter()
            .map(|(name, has_children)| {
                let child = format!("{}/{}", parent.trim_end_matches('/'), name);
                // System-critical files: can never be deleted
                let locked = storage::is_protected_path(&home, &child);
                json!({
                    "name": name,
                    "hasChildren": has_children,
                    "excludable": is_excludable_name(&name) && !locked,
                    "locked": locked,
                })
            })
            .collect();
        cxx_qt_lib::QString::from(&Value::Array(items).to_string())
    }

    /// Calculates the size of each subfolder in `path` in the background
    /// and sends it as it becomes available (folderSizeReady signal)
    pub fn request_folder_sizes(self: Pin<&mut Self>, path: &cxx_qt_lib::QString, show_hidden: bool) {
        let parent = path.to_string();
        let qt_thread = self.qt_thread();

        std::thread::spawn(move || {
            for (name, _) in list_dirs(&parent, show_hidden) {
                let child = format!("{}/{}", parent.trim_end_matches('/'), name);
                let dir = match safe_abs(&child) {
                    Some(p) => p,
                    None => continue,
                };
                let bytes = dir_size(&dir);
                qt_thread
                    .queue(move |mut qobject| {
                        let path_q = cxx_qt_lib::QString::from(&child);
                        qobject.as_mut().folderSizeReady(&path_q, bytes as i64);
                    })
                    .ok();
            }
        });
    }

    /// Size of a single folder, calculated in the background
    pub fn request_path_size(self: Pin<&mut Self>, path: &cxx_qt_lib::QString) {
        let target = path.to_string();
        let qt_thread = self.qt_thread();

        std::thread::spawn(move || {
            let dir = match safe_abs(&target) {
                Some(p) => p,
                None => return,
            };
            let bytes = dir_size(&dir);
            qt_thread
                .queue(move |mut qobject| {
                    let path_q = cxx_qt_lib::QString::from(&target);
                    qobject.as_mut().folderSizeReady(&path_q, bytes as i64);
                })
                .ok();
        });
    }

    /// Estimates (using the helper) how much would be saved, after excluding specified folders
    pub fn measure_data(self: Pin<&mut Self>, excluded_folders: &cxx_qt_lib::QString) {
        let excluded = drop_protected(parse_excluded(&excluded_folders.to_string()));
        let data_size = self.data_size.clone();
        let qt_thread = self.qt_thread();

        std::thread::spawn(move || {
            eprintln!("Excluded folders: {:?}", excluded);
            let mut analyzer = DiskAnalyzer::new();
            for folder in &excluded {
                analyzer.add_exclusion(folder);
            }
            match analyzer.calculate_backup_size(Path::new(storage::SYSTEM_BACKUP_DIR)) {
                Ok(bytes) => {
                    eprintln!("Data to back up: {}", format_bytes(bytes));
                    data_size.store(bytes, Ordering::SeqCst);
                    emit_measured(&qt_thread, bytes as i64, String::new());
                }
                Err(e) => {
                    eprintln!("Failed to measure the data: {}", e);
                    emit_measured(&qt_thread, 0, e.to_string());
                }
            }
        });
    }

    /// Specify a location (folder that will contain the image) for the measured data
    pub fn evaluate_destination(
        self: Pin<&mut Self>,
        path: &cxx_qt_lib::QString,
    ) -> cxx_qt_lib::QString {
        let data = self.data_size.load(Ordering::SeqCst);
        let ev = storage::evaluate(Path::new(&path.to_string()), data);
        eprintln!(
            "Evaluate {}: needed {}, available {}, existing {}, OK: {}",
            path.to_string(),
            format_bytes(ev.needed),
            format_bytes(ev.available),
            format_bytes(ev.existing),
            ev.enough
        );
        let value = json!({
            "problem": ev.problem.map(|p| p.code()).unwrap_or(""),
            "needed": ev.needed,
            "available": ev.available,
            "existing": ev.existing,
            "enough": ev.enough,
        });
        cxx_qt_lib::QString::from(&value.to_string())
    }

    pub fn list_media(self: Pin<&mut Self>) -> cxx_qt_lib::QString {
        let data = self.data_size.load(Ordering::SeqCst);
        cxx_qt_lib::QString::from(&Value::Array(media::list(data)).to_string())
    }

    pub fn mount_media(self: Pin<&mut Self>, device: &cxx_qt_lib::QString) {
        let device = device.to_string();
        let qt_thread = self.qt_thread();

        std::thread::spawn(move || {
            let (mountpoint, error) = match media::mount(&device) {
                Ok(mp) => (mp, String::new()),
                Err(e) => (String::new(), e),
            };
            qt_thread
                .queue(move |mut qobject| {
                    let device_q = cxx_qt_lib::QString::from(&device);
                    let mp_q = cxx_qt_lib::QString::from(&mountpoint);
                    let err_q = cxx_qt_lib::QString::from(&error);
                    qobject.as_mut().mediaMounted(&device_q, &mp_q, &err_q);
                })
                .ok();
        });
    }

    /// Launch the root helper (a single pkexec) and monitor its progress
    pub fn start_backup(
        mut self: Pin<&mut Self>,
        destination: &cxx_qt_lib::QString,
        excluded_folders: &cxx_qt_lib::QString,
        keep: i32,
        persist: bool,
    ) {
        if self.is_running {
            return;
        }

        let data = self.data_size.load(Ordering::SeqCst);
        if data == 0 {
            let msg = cxx_qt_lib::QString::from("Run the space analysis first.");
            self.as_mut().backupFinished(false, &msg);
            return;
        }
        let size = storage::image_size_for(data);

        let dest = destination.to_string();
        let excluded = drop_protected(parse_excluded(&excluded_folders.to_string()));
        let keep = keep.clamp(1, 100);

        //The selected location, retention, and exclusions are saved
        if persist {
            let mut config = ConfigManager::load().unwrap_or_default();
            config.dest_path = PathBuf::from(&dest);
            config.keep_snapshots = keep as u32;
            config.excluded_paths = excluded.clone();
            let _ = ConfigManager::save(&config);
        }

        let canceled = self.backup_canceled.clone();
        canceled.store(false, Ordering::SeqCst);
        let qt_thread = self.qt_thread();

        self.as_mut().set_is_running(true);
        self.as_mut().set_progress_percent(0);
        self.as_mut().set_current_file(cxx_qt_lib::QString::from("preparing"));
        self.as_mut().set_backup_status(cxx_qt_lib::QString::from("Starting backup..."));

        std::thread::spawn(move || {
            let helper = match storage::helper_path() {
                Some(h) => h,
                None => {
                    report(&qt_thread, Outcome::Failed("Helper script not found (astroarch-backup-helper).".to_string()));
                    return;
                }
            };

            let cancel_file = cancel_file_path();
            let _ = std::fs::remove_file(&cancel_file);

            let mut cmd = StdCommand::new("pkexec");
            cmd.arg(&helper)
                .arg("backup")
                .arg("--dest")
                .arg(&dest)
                .arg("--size")
                .arg(size.to_string())
                .arg("--keep")
                .arg(keep.to_string())
                .arg("--cancel-file")
                .arg(&cancel_file);
            for folder in &excluded {
                cmd.arg("--exclude").arg(folder);
            }
            cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

            eprintln!(
                "Running: pkexec {} backup --dest {} --size {} --keep {}",
                helper.display(),
                dest,
                size,
                keep
            );

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    report(&qt_thread, Outcome::Failed(format!("Failed to start pkexec: {}", e)));
                    return;
                }
            };

            let helper_error = Arc::new(Mutex::new(String::new()));
            let stderr_text = Arc::new(Mutex::new(String::new()));

            let out_handle = child.stdout.take().map(|out| {
                let qt = qt_thread.clone();
                let herr = helper_error.clone();
                std::thread::spawn(move || {
                    read_records(out, |rec| {
                        let rec = rec.trim();
                        if let Some(stage) = rec.strip_prefix("STAGE:") {
                            let stage = stage.to_string();
                            qt.queue(move |mut qobject| {
                                // 99% until the backup is complete, 100% when it's finished
                                let progress = match stage.as_str() {
                                    "boot" | "snapshot" | "verifying" | "pruning" | "shrinking" => Some(99),
                                    "finishing" => Some(100),
                                    _ => None,
                                };
                                if let Some(p) = progress {
                                    qobject.as_mut().set_progress_percent(p);
                                }
                                qobject.as_mut().set_current_file(cxx_qt_lib::QString::from(&stage));
                            })
                            .ok();
                        } else if let Some(err) = rec.strip_prefix("ERROR:") {
                            // ERROR:<code>:<message>
                            let msg = err.splitn(2, ':').nth(1).unwrap_or(err).to_string();
                            eprintln!("helper error: {}", msg);
                            *herr.lock().unwrap() = msg;
                        } else if rec.starts_with("WARN:") {
                            eprintln!("helper: {}", rec);
                        } else if let Some(percent) = parse_progress(rec) {
                            // Never reaches 100% during the copy: the backup isn't finished
                            qt.queue(move |mut qobject| {
                                qobject.as_mut().set_progress_percent(percent.min(99));
                            })
                            .ok();
                        }
                    });
                })
            });

            let err_handle = child.stderr.take().map(|err| {
                let text = stderr_text.clone();
                std::thread::spawn(move || {
                    read_records(err, |rec| {
                        let mut t = text.lock().unwrap();
                        if t.len() < 4000 {
                            t.push_str(rec);
                            t.push('\n');
                        }
                    });
                })
            });

            let status = child.wait();
            if let Some(h) = out_handle {
                let _ = h.join();
            }
            if let Some(h) = err_handle {
                let _ = h.join();
            }
            let _ = std::fs::remove_file(&cancel_file);

            let outcome = match status {
                Ok(st) if st.success() => {
                    let mut config = ConfigManager::load().unwrap_or_default();
                    config.first_backup = false;
                    let _ = ConfigManager::save(&config);
                    Outcome::Success
                }
                Ok(_) if canceled.load(Ordering::SeqCst) => Outcome::Canceled,
                Ok(st) => {
                    let helper_msg = helper_error.lock().unwrap().clone();
                    let stderr_msg = stderr_text.lock().unwrap().clone();
                    let message = failure_message(st.code(), &helper_msg, &stderr_msg);
                    eprintln!("Backup failed (exit code {:?}): {}", st.code(), message);
                    if !stderr_msg.trim().is_empty() {
                        eprintln!("helper stderr:\n{}", stderr_msg);
                    }
                    Outcome::Failed(message)
                }
                Err(e) => Outcome::Failed(format!("Failed to wait for the backup process: {}", e)),
            };
            report(&qt_thread, outcome);
        });
    }

    /// Requests that the backup be stopped. The helper runs as root: it cannot
    /// be terminated; instead, cancellation is signaled via a file (without a second
    /// password). Existing snapshots are never affected. The helper
    /// completes its cleanup even if the application closes in the meantime
    pub fn cancel_backup(self: Pin<&mut Self>) {
        eprintln!("Canceling backup...");
        self.backup_canceled.store(true, Ordering::SeqCst);
        let _ = std::fs::write(cancel_file_path(), b"cancel");
    }
}

impl qobject::BackupManager {
    pub fn has_backup(self: Pin<&mut Self>, path: &cxx_qt_lib::QString) -> bool {
        storage::existing_image_size(Path::new(&path.to_string())).is_some()
    }

    pub fn list_snapshots(self: Pin<&mut Self>, destination: &cxx_qt_lib::QString) {
        let dest = destination.to_string();
        let qt_thread = self.qt_thread();

        std::thread::spawn(move || {
            let (json, error) = match run_helper_list_snapshots(&dest) {
                Ok(j) => (j, String::new()),
                Err(e) => ("[]".to_string(), e),
            };
            qt_thread
                .queue(move |mut qobject| {
                    let json_q = cxx_qt_lib::QString::from(&json);
                    let err_q = cxx_qt_lib::QString::from(&error);
                    qobject.as_mut().snapshotsListed(&json_q, &err_q);
                })
                .ok();
        });
    }

    /// Starts the full restore. The helper checks everything (no changes),
    /// then delegates the task to a systemd service and exits: the desktop shuts down
    /// a few seconds later, and the Raspberry Pi reboots on its own when the process is complete
    pub fn start_restore(
        self: Pin<&mut Self>,
        destination: &cxx_qt_lib::QString,
        snapshot: &cxx_qt_lib::QString,
    ) {
        let dest = destination.to_string();
        let snap = snapshot.to_string();
        let qt_thread = self.qt_thread();

        std::thread::spawn(move || match run_helper_restore(&dest, &snap) {
            Ok(()) => {
                qt_thread
                    .queue(move |mut qobject| {
                        qobject.as_mut().restoreStarted();
                    })
                    .ok();
            }
            Err(message) => {
                eprintln!("Restore not started: {}", message);
                qt_thread
                    .queue(move |mut qobject| {
                        let err_q = cxx_qt_lib::QString::from(&message);
                        qobject.as_mut().restoreFailed(&err_q);
                    })
                    .ok();
            }
        });
    }
}

/// Remove the folders that are essential to the system from the exclusions
fn drop_protected(paths: Vec<String>) -> Vec<String> {
    let home = home_dir().to_string_lossy().to_string();
    paths
        .into_iter()
        .filter(|p| {
            let protected = storage::is_protected_path(&home, p);
            if protected {
                eprintln!("Ignoring protected folder (cannot be excluded): {}", p);
            }
            !protected
        })
        .collect()
}

fn helper_error_text(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("ERROR:") {
            return Some(rest.splitn(2, ':').nth(1).unwrap_or(rest).to_string());
        }
    }
    None
}

/// Runs the helper via pkexec and returns its standard output (Err = readable message)
fn run_helper(args: &[&str]) -> Result<String, String> {
    let helper = storage::helper_path()
        .ok_or_else(|| "Helper script not found (astroarch-backup-helper).".to_string())?;
    let out = StdCommand::new("pkexec")
        .arg(&helper)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Failed to start pkexec: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let helper_err = helper_error_text(&stdout).unwrap_or_default();
        Err(failure_message(out.status.code(), &helper_err, &stderr))
    }
}

fn run_helper_list_snapshots(dest: &str) -> Result<String, String> {
    let stdout = run_helper(&["snapshots", "--dest", dest])?;
    let items: Vec<Value> = stdout
        .lines()
        .filter_map(|l| l.trim().strip_prefix("SNAPSHOT:"))
        .filter_map(|rest| {
            let mut parts = rest.splitn(2, ':');
            let name = parts.next()?;
            let boot = parts.next()? == "1";
            Some(json!({ "name": name, "boot": boot }))
        })
        .collect();
    Ok(Value::Array(items).to_string())
}

fn run_helper_restore(dest: &str, snapshot: &str) -> Result<(), String> {
    let stdout = run_helper(&[
        "restore",
        "--dest",
        dest,
        "--snapshot",
        snapshot,
        "--confirm",
        "RESTORE",
    ])?;
    if stdout.lines().any(|l| l.trim() == "RESULT:restore-started") {
        Ok(())
    } else {
        Err("The restore did not start.".to_string())
    }
}

fn emit_measured(qt_thread: &BackupThread, bytes: i64, error: String) {
    qt_thread
        .queue(move |mut qobject| {
            let err_q = cxx_qt_lib::QString::from(&error);
            qobject.as_mut().dataMeasured(bytes, &err_q);
        })
        .ok();
}

fn report(qt_thread: &BackupThread, outcome: Outcome) {
    qt_thread
        .queue(move |mut qobject| {
            qobject.as_mut().set_is_running(false);
            match outcome {
                Outcome::Success => {
                    qobject.as_mut().set_backup_status(cxx_qt_lib::QString::from("Backup complete!"));
                    qobject.as_mut().set_progress_percent(100);
                    let empty = cxx_qt_lib::QString::from("");
                    qobject.as_mut().backupFinished(true, &empty);
                }
                Outcome::Canceled => {
                    qobject.as_mut().set_backup_status(cxx_qt_lib::QString::from("Backup canceled"));
                    qobject.as_mut().set_progress_percent(0);
                    qobject.as_mut().backupCanceled();
                }
                Outcome::Failed(message) => {
                    qobject.as_mut().set_backup_status(cxx_qt_lib::QString::from("Error while saving"));
                    qobject.as_mut().set_progress_percent(0);
                    let msg_q = cxx_qt_lib::QString::from(&message);
                    qobject.as_mut().backupFinished(false, &msg_q);
                }
            }
        })
        .ok();
}

fn failure_message(code: Option<i32>, helper_error: &str, stderr: &str) -> String {
    match code {
        // pkexec : 126 = not allowed, 127 = password window closed
        Some(126) | Some(127) => "Authorization was refused or cancelled.".to_string(),
        _ => {
            if !helper_error.is_empty() {
                helper_error.to_string()
            } else if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                format!("The backup failed (exit code {:?}).", code)
            }
        }
    }
}

/// File through which the app notifies the root helper of the cancellation
fn cancel_file_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("astroarch-backup.cancel");
        }
    }
    let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
    PathBuf::from(format!("/tmp/astroarch-backup-{}.cancel", uid))
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".to_string()))
}

/// Valid absolute path: no “..”, no empty components, and located in the
/// user folder or the shared data folder
fn safe_abs(path: &str) -> Option<PathBuf> {
    if !path.starts_with('/') {
        return None;
    }
    for comp in path.split('/').skip(1) {
        if comp.is_empty() || comp == "." || comp == ".." {
            return None;
        }
    }
    let home = home_dir().to_string_lossy().trim_end_matches('/').to_string();
    let within = |root: &str| path == root || path.starts_with(&format!("{}/", root));
    if (!home.is_empty() && within(&home)) || within(storage::SHARED_DATA_DIR) {
        Some(PathBuf::from(path))
    } else {
        None
    }
}

/// A directory can only be excluded if its name does not contain any
/// special characters for rsync (same rule as the helper)
fn is_excludable_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(|c: char| matches!(c, '*' | '?' | '[' | ']' | '\\' | '\n' | '/'))
}

/// Subfolders of `path` (absolute): (name, has subfolders), sorted without
/// regard to case. Symbolic links are ignored (no loops)
fn list_dirs(path: &str, show_hidden: bool) -> Vec<(String, bool)> {
    let base = match safe_abs(path) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let visible = |name: &str| show_hidden || !name.starts_with('.');
    let mut items: Vec<(String, bool)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !visible(&name) {
                continue;
            }
            match entry.file_type() {
                Ok(t) if t.is_dir() => {}
                _ => continue,
            }
            let has_children = std::fs::read_dir(entry.path())
                .map(|rd| {
                    rd.flatten().any(|e| {
                        e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                            && visible(&e.file_name().to_string_lossy())
                    })
                })
                .unwrap_or(false);
            items.push((name, has_children));
        }
    }
    items.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    items
}

/// Size (apparent bytes) of a folder, without traversing other file systems
fn dir_size(path: &Path) -> u64 {
    // "/" final : suit un éventuel lien symbolique vers le dossier
    let target = format!("{}/", path.to_string_lossy().trim_end_matches('/'));
    match StdCommand::new("du").args(["-sb", "-x", "--"]).arg(target).output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or(0),
        Err(_) => 0,
    }
}

/// Excluded folders: one absolute path per line
fn parse_excluded(input: &str) -> Vec<String> {
    input
        .split('\n')
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Reads a stream by splitting it on \n AND \r (rsync updates its
/// progress bar with \r characters)
fn read_records<R: Read>(mut reader: R, mut on_record: impl FnMut(&str)) {
    let mut buf = [0u8; 4096];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                for &byte in &buf[..n] {
                    if byte == b'\n' || byte == b'\r' {
                        if !pending.is_empty() {
                            on_record(&String::from_utf8_lossy(&pending));
                            pending.clear();
                        }
                    } else {
                        pending.push(byte);
                    }
                }
            }
        }
    }
    if !pending.is_empty() {
        on_record(&String::from_utf8_lossy(&pending));
    }
}

/// Overall percentage for a line `--info=progress2`: “  1.2G  45%  50MB/s ...”
fn parse_progress(record: &str) -> Option<i32> {
    let idx = record.find('%')?;
    let token = record[..idx].split_whitespace().last()?;
    let value: i32 = token.parse().ok()?;
    if (0..=100).contains(&value) {
        Some(value)
    } else {
        None
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_abs_limits_to_known_roots() {
        std::env::set_var("HOME", "/home/astronaut");
        assert!(safe_abs("/home/astronaut").is_some());
        assert!(safe_abs("/home/astronaut/Documents/Astro").is_some());
        assert!(safe_abs("/srv/astro-data").is_some());
        assert!(safe_abs("/srv/astro-data/raw").is_some());
        assert!(safe_abs("/etc").is_none());
        assert!(safe_abs("/home/other").is_none());
        assert!(safe_abs("/home/astronaut/../other").is_none());
        assert!(safe_abs("/srv/astro-data2").is_none());
        assert!(safe_abs("relative/path").is_none());
        assert!(safe_abs("/home/astronaut//x").is_none());
    }

    #[test]
    fn excludable_names() {
        assert!(is_excludable_name("Mes Photos"));
        assert!(is_excludable_name(".cache"));
        assert!(!is_excludable_name("data[1]"));
        assert!(!is_excludable_name("a*b"));
    }

    #[test]
    fn parse_excluded_lines() {
        assert_eq!(
            parse_excluded("/home/a/Documents\n /srv/astro-data/ \n\n/"),
            vec!["/home/a/Documents".to_string(), "/srv/astro-data".to_string()]
        );
    }

    #[test]
    fn helper_error_text_extracts_message() {
        assert_eq!(
            helper_error_text("STAGE:preparing\nERROR:42:another disk\n"),
            Some("another disk".to_string())
        );
        assert_eq!(helper_error_text("RESULT:ok"), None);
    }

    #[test]
    fn parse_progress_lines() {
        assert_eq!(parse_progress("  1.23G  45%   50.00MB/s    0:01:23"), Some(45));
        assert_eq!(parse_progress("  12,345  100%  1.00MB/s"), Some(100));
        assert_eq!(parse_progress("no percent here"), None);
    }

    #[test]
    fn format_bytes_gib() {
        assert_eq!(format_bytes(15 * 1024 * 1024 * 1024), "15.00 GiB");
    }
}

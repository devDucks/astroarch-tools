//! The system backup and the full restore from a terminal, without the window:
//!   astroarch-backup --system-backup                 (zsh command `backup-astroarch`)
//!   astroarch-backup --system-restore [--dest DIR]   (zsh command `restore-astroarch`)
//!
//! Same helper, same image and snapshots as the window. The settings are the ones the window
//! saves in ~/.backup_dest (location, folders left out, snapshots kept): they are chosen in
//! the window (Control Center, Data backup, System), the terminal only runs them.
//! The helper runs through sudo, which asks the password in the terminal (pkexec refuses
//! over SSH). Ctrl+C stops a backup: the helper cleans up, the snapshots already made stay.

use crate::bridge::{
    cancel_file_path, drop_protected, failure_message, format_bytes, parse_progress, parse_snapshots,
    read_records, run_helper, run_helper_restore,
};
use crate::config::{BackupConfig, ConfigManager};
use crate::storage;

use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// A safety backup before a restore keeps every snapshot (as the window does): the one being
/// restored is never pruned
const SAFETY_KEEP: u32 = 100;

/// `--system-backup`: one system backup with the saved settings. Exit code 0 when done
pub fn system_backup() -> i32 {
    storage::use_sudo();
    let Some(config) = saved_config() else {
        return 1;
    };
    let dest = config.dest_path.to_string_lossy().to_string();
    let excluded = drop_protected(config.excluded_paths.clone());
    println!("System backup to {}", dest);
    print_left_out(&excluded);
    println!("Snapshots kept: {}", config.keep_clamped());
    if !authorize() {
        return 1;
    }
    match run_backup(&dest, &excluded, config.keep_clamped()) {
        Ok(snapshot) => {
            println!("Backup complete: snapshot {}", snapshot);
            0
        }
        Err(message) => {
            eprintln!("{}", message);
            1
        }
    }
}

/// `--system-restore [--dest DIR]`: choose a snapshot, optional safety backup, RESTORE, then
/// the helper restores the whole system and restarts the computer
pub fn system_restore(dest_arg: Option<String>) -> i32 {
    storage::use_sudo();
    let config = ConfigManager::load().unwrap_or_default();
    let dest = dest_arg.unwrap_or_else(|| config.dest_path.to_string_lossy().to_string());
    if storage::existing_image_size(Path::new(&dest)).is_none() {
        eprintln!("No system backup in {}.", dest);
        eprintln!("Another place: restore-astroarch --dest FOLDER (the folder that holds {}).", storage::IMAGE_FILE_NAME);
        return 1;
    }
    if !authorize() {
        return 1;
    }

    let listed = match run_helper(&["snapshots", "--dest", &dest]) {
        Ok(out) => parse_snapshots(&out, None),
        Err((_, e)) => {
            eprintln!("The backup could not be read: {}", e);
            return 1;
        }
    };
    let names: Vec<String> = listed
        .iter()
        .filter(|s| s["restorable"].as_bool() == Some(true) && s["boot"].as_bool() == Some(true))
        .filter_map(|s| s["name"].as_str().map(str::to_string))
        .collect();
    if names.is_empty() {
        eprintln!("No snapshot in {} can restore this whole system (made on this kind of computer, with its boot partition).", dest);
        return 1;
    }

    println!("System backups in {}, newest first:", dest);
    for (i, name) in names.iter().enumerate() {
        println!("  {}) {}", i + 1, readable_name(name));
    }
    let answer = ask("Number of the backup to restore (Enter: stop): ");
    let Some(snapshot) = answer.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| names.get(i)) else {
        println!("Nothing was restored.");
        return 0;
    };

    println!();
    println!("The whole system is replaced by its state of {}:", readable_name(snapshot));
    println!("  - everything changed since then is lost: files, installed programs and updates, settings;");
    println!("  - any folders left out of the backups are emptied (the folders themselves stay);");
    println!("  - the restore runs in text mode on the computer's screen for several minutes, then the");
    println!("    computer restarts by itself (an SSH connection closes);");
    println!("  - do not switch it off or unplug it meanwhile: a power cut can leave the system unusable;");
    println!("  - it only works on the disk the backup was made from: otherwise it is refused before anything changes.");
    println!("The backups in this place and the drives mounted separately are not touched.");
    let excluded = drop_protected(config.excluded_paths.clone());
    if !excluded.is_empty() {
        println!("Left out of the system backups: {}. What they hold is not in the safety backup either, so it is lost.", excluded.join(", "));
    }
    println!();
    let safety = !matches!(ask("First a safety backup of the system as it is now, to undo the restore? [Y/n] ").to_lowercase().as_str(), "n" | "no");
    if ask("Type RESTORE to confirm: ") != "RESTORE" {
        println!("Nothing was restored.");
        return 0;
    }

    if safety {
        println!("Safety backup first:");
        if let Err(message) = run_backup(&dest, &excluded, SAFETY_KEEP) {
            eprintln!("The safety backup failed, so nothing was restored: {}", message);
            return 1;
        }
    }
    println!("Checking the backup and preparing the restore…");
    match run_helper_restore(&dest, snapshot, false) {
        Ok(()) => {
            println!("The restore has started. It runs in text mode on the computer's screen, then the computer restarts by itself. Do not switch it off.");
            0
        }
        Err((_, message)) => {
            eprintln!("The restore did not start, nothing was changed: {}", message);
            1
        }
    }
}

/// The settings saved by the window, None (with a message) when there are none yet
fn saved_config() -> Option<BackupConfig> {
    let saved = ConfigManager::config_path().map(|p| p.exists()).unwrap_or(false);
    if !saved {
        eprintln!("The system backup is not set up yet: choose where it goes and what it leaves out in the");
        eprintln!("AstroArch Control Center (Data backup, System), then run it here or there.");
        return None;
    }
    match ConfigManager::load() {
        Ok(config) => Some(config),
        Err(e) => {
            eprintln!("The backup settings (~/.backup_dest) cannot be read: {}", e);
            None
        }
    }
}

fn print_left_out(excluded: &[String]) {
    if excluded.is_empty() {
        println!("Left out: nothing");
    } else {
        println!("Left out: {}", excluded.join(", "));
    }
}

/// The password once at the start (sudo keeps it a few minutes), so that it is not asked in the
/// middle of the progress
fn authorize() -> bool {
    println!("The backup runs as administrator: sudo may ask your password.");
    match Command::new("sudo").arg("-v").status() {
        Ok(st) if st.success() => true,
        _ => {
            eprintln!("Authorization was refused or cancelled.");
            false
        }
    }
}

/// Measures, checks the place, then runs the helper's backup with its progress on one line.
/// Ok(snapshot name)
fn run_backup(dest: &str, excluded: &[String], keep: u32) -> Result<String, String> {
    let helper = storage::helper_path().ok_or("Helper script not found (astroarch-backup-helper).")?;

    println!("Measuring what to back up…");
    let mut args = vec!["measure".to_string(), "--dest".to_string(), storage::SYSTEM_BACKUP_DIR.to_string()];
    for folder in excluded {
        args.push("--exclude".to_string());
        args.push(folder.clone());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_helper(&refs).map_err(|(_, e)| format!("The measure failed: {}", e))?;
    let data: u64 = out
        .lines()
        .find_map(|l| l.trim().strip_prefix("RESULT:size:"))
        .and_then(|v| v.trim().parse().ok())
        .ok_or("The measure gave no size.")?;

    let ev = storage::evaluate(Path::new(dest), data);
    if let Some(problem) = ev.problem {
        return Err(format!("{} cannot hold the backup: {}.", dest, problem_text(problem.code())));
    }
    if !ev.enough {
        return Err(format!(
            "Not enough space in {}: {} needed, {} free.",
            dest,
            format_bytes(ev.needed),
            format_bytes(ev.available)
        ));
    }
    println!("{} in the system; only what changed since the last backup is copied.", format_bytes(data));
    let size = storage::image_size_for(data);

    let cancel_file = cancel_file_path();
    let _ = std::fs::remove_file(&cancel_file);
    let mut cmd = storage::helper_command(&helper);
    cmd.arg("backup")
        .arg("--dest")
        .arg(dest)
        .arg("--size")
        .arg(size.to_string())
        .arg("--keep")
        .arg(keep.to_string())
        .arg("--cancel-file")
        .arg(&cancel_file);
    for folder in excluded {
        cmd.arg("--exclude").arg(folder);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    // Ctrl+C reaches the helper (it cleans up and stops); this program waits for it instead of
    // ending first, which would cut the helper's output in the middle of its cleanup
    keep_running_on_ctrl_c();
    let mut child = cmd.spawn().map_err(|e| {
        default_ctrl_c();
        format!("Failed to start sudo: {}", e)
    })?;

    let helper_error = Arc::new(Mutex::new(String::new()));
    let snapshot = Arc::new(Mutex::new(String::new()));
    let last_stage = Arc::new(Mutex::new(String::new()));
    let stderr_text = Arc::new(Mutex::new(String::new()));
    let out_handle = child.stdout.take().map(|out| {
        let herr = helper_error.clone();
        let snap = snapshot.clone();
        let stage_seen = last_stage.clone();
        std::thread::spawn(move || {
            // The progress line is rewritten in place, only when its text changes
            let mut last = String::new();
            let show = |last: &mut String, text: String| {
                if *last != text {
                    print!("\r\x1b[K{}", text);
                    let _ = std::io::stdout().flush();
                    *last = text;
                }
            };
            read_records(out, |rec| {
                let rec = rec.trim();
                if let Some(stage) = rec.strip_prefix("STAGE:") {
                    *stage_seen.lock().unwrap() = stage.to_string();
                    show(&mut last, stage_text(stage).to_string());
                } else if let Some(name) = rec.strip_prefix("RESULT:ok:") {
                    *snap.lock().unwrap() = name.to_string();
                } else if let Some(err) = rec.strip_prefix("ERROR:") {
                    *herr.lock().unwrap() = err.splitn(2, ':').nth(1).unwrap_or(err).to_string();
                } else if rec.starts_with("WARN:") {
                    // The helper's diagnostics (room, shrink-check...): the window does not
                    // show them either
                } else if let Some(percent) = parse_progress(rec) {
                    show(&mut last, format!("Copying… {}%", percent.min(99)));
                }
            });
            println!();
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
    default_ctrl_c();
    if let Some(h) = out_handle {
        let _ = h.join();
    }
    if let Some(h) = err_handle {
        let _ = h.join();
    }
    let _ = std::fs::remove_file(&cancel_file);

    match status {
        Ok(st) if st.success() => {
            let mut config = ConfigManager::load().unwrap_or_default();
            config.first_backup = false;
            let _ = ConfigManager::save(&config);
            Ok(snapshot.lock().unwrap().clone())
        }
        // 130: canceled, 143: stopped by Ctrl+C (its INT trap)
        Ok(st) if matches!(st.code(), Some(130) | Some(143)) => Err(stopped_text(&last_stage.lock().unwrap()).to_string()),
        Ok(st) => Err(failure_message(st.code(), &helper_error.lock().unwrap(), &stderr_text.lock().unwrap())),
        Err(e) => Err(format!("Failed to wait for the backup process: {}", e)),
    }
}

/// The helper's stages, in words (the window shows the same steps)
fn stage_text(stage: &str) -> &str {
    match stage {
        "preparing" => "Preparing…",
        "copying" => "Copying…",
        "boot" => "Copying the boot partition…",
        "snapshot" => "Making the snapshot…",
        "verifying" => "Verifying…",
        "pruning" => "Removing the oldest snapshots…",
        "shrinking" => "Shrinking the backup file…",
        "finishing" => "Finishing…",
        other => other,
    }
}

/// What a stop left, by the last stage the helper reached: from "verifying" on, the new snapshot
/// is already made (see cmd_backup in the helper)
fn stopped_text(stage: &str) -> &'static str {
    match stage {
        "verifying" | "pruning" | "shrinking" | "finishing" => {
            "Backup stopped after its snapshot was made: the new snapshot was not verified, and no older snapshot was removed."
        }
        _ => "Backup stopped before its snapshot: the snapshots made before are untouched.",
    }
}

fn problem_text(code: &str) -> &'static str {
    match code {
        "missing" => "the folder does not exist (is the drive plugged in and mounted?)",
        "readonly" => "it is read-only",
        "fat32" => "FAT32 cannot hold files over 4 GB",
        "network" => "a network folder cannot hold the system backup",
        "temporary" => "it is in memory, lost at the next start",
        _ => "unsuitable place",
    }
}

/// 2026-11-03_072950 → 2026-11-03 07:29:50
fn readable_name(name: &str) -> String {
    match name.split_once('_') {
        Some((day, time)) if time.len() == 6 => format!("{} {}:{}:{}", day, &time[0..2], &time[2..4], &time[4..6]),
        _ => name.to_string(),
    }
}

/// One answer from the terminal, trimmed ("" at the end of the input)
fn ask(question: &str) -> String {
    print!("{}", question);
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim().to_string()
}

extern "C" fn on_ctrl_c(_: libc::c_int) {}

/// A handler that does nothing (not SIG_IGN: an ignored signal stays ignored in the helper,
/// which then could not be stopped; a handler is reset in the programs this one starts)
fn keep_running_on_ctrl_c() {
    unsafe {
        libc::signal(libc::SIGINT, on_ctrl_c as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
}

/// Ctrl+C ends this program again (after the backup: the restore questions)
fn default_ctrl_c() {
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_snapshot_names() {
        assert_eq!(readable_name("2026-11-03_072950"), "2026-11-03 07:29:50");
        assert_eq!(readable_name("odd"), "odd");
    }

    #[test]
    fn a_stop_after_the_snapshot_says_so() {
        assert!(stopped_text("copying").contains("before its snapshot"));
        assert!(stopped_text("").contains("before its snapshot"));
        assert!(stopped_text("verifying").contains("after its snapshot"));
    }

    #[test]
    fn every_stage_has_words() {
        for stage in ["preparing", "copying", "boot", "snapshot", "verifying", "pruning", "shrinking", "finishing"] {
            assert_ne!(stage_text(stage), stage);
        }
    }
}

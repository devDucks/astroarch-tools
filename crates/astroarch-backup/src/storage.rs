#![allow(dead_code)]

//! Backup location: checks on the host file system,
//! calculation of the Btrfs image file size, location of the root helper.
//!
//! Only READ operations here (findmnt, df, stat): no privileged operations.
//! The same checks are performed again by the helper (scripts/astroarch-backup-helper.sh)
//! at the time of the backup, as only the helper’s results are considered authoritative

use std::path::{Path, PathBuf};
use std::process::Command;

pub const MIB: u64 = 1024 * 1024;
pub const GIB: u64 = 1024 * MIB;

/// Required margin of free space around the image (same as the helper)
pub const HOST_MARGIN: u64 = 128 * MIB;

/// Name of the Btrfs image file created in the destination folder
pub const IMAGE_FILE_NAME: &str = "astroarch-backup.btrfs";

/// MANDATORY destination on the main disk (FHS: /var/backups)
pub const SYSTEM_BACKUP_DIR: &str = "/var/backups";

/// Data folder shared among AstroArch users (suggested as an alternative if one already exists)
pub const SHARED_DATA_DIR: &str = "/srv/astro-data";

/// Files in the personal folder that are essential for proper operation: they (and their
/// contents) cannot be excluded from the backup. Same list as the helper
pub const PROTECTED_HOME_DIRS: [&str; 5] = ["Desktop", ".astroarch", ".config", ".local", ".oh-my-zsh"];

/// Is `path` one of these folders, or located inside one of them?
pub fn is_protected_path(home: &str, path: &str) -> bool {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return false;
    }
    PROTECTED_HOME_DIRS.iter().any(|d| {
        let root = format!("{}/{}", home, d);
        path == root || path.starts_with(&format!("{}/", root))
    })
}

/// Subfolder used on external media: <mount point>/astroarch/backups
pub const MEDIA_SUBDIR: &str = "astroarch/backups";

const INSTALLED_HELPER: &str = "/usr/lib/astroarch-backup/astroarch-backup-helper";
const DEV_HELPER_NAME: &str = "astroarch-backup-helper.sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationProblem {
    /// The location does not exist or is not a folder
    Missing,
    /// File system mounted as read-only
    ReadOnly,
    /// FAT/FAT32: Files are limited to 4 GB; the image cannot be stored there
    Fat32,
    /// NFS, SMB, sshfs... : unreliable for an image mounted in a loop
    Network,
    /// tmpfs/ramfs : lost upon restart
    Temporary,
}

impl DestinationProblem {
    /// Code passed to QML, which displays the corresponding translated message
    pub fn code(self) -> &'static str {
        match self {
            DestinationProblem::Missing => "missing",
            DestinationProblem::ReadOnly => "readonly",
            DestinationProblem::Fat32 => "fat32",
            DestinationProblem::Network => "network",
            DestinationProblem::Temporary => "temporary",
        }
    }
}

/// The closest existing folder above `path` (or `path` itself)
pub fn existing_ancestor(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    loop {
        if p.is_dir() {
            return p;
        }
        let next = p.parent().map(|x| x.to_path_buf());
        match next {
            Some(n) => p = n,
            None => return PathBuf::from("/"),
        }
    }
}

/// Absolute path even if the end does not yet exist: we resolve the closest
/// existing ancestor (including symbolic links) and add the rest
pub fn resolve_dest(path: &Path) -> PathBuf {
    let ancestor = existing_ancestor(path);
    let canon = std::fs::canonicalize(&ancestor).unwrap_or_else(|_| ancestor.clone());
    match path.strip_prefix(&ancestor) {
        Ok(rest) => canon.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// (file system type, mount options) of the location
fn filesystem_info(path: &Path) -> Option<(String, String)> {
    let out = Command::new("findmnt")
        .args(["-n", "-o", "FSTYPE,OPTIONS", "-T"])
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    let mut parts = line.splitn(2, ' ');
    let fstype = parts.next()?.to_string();
    let options = parts.next().unwrap_or("").to_string();
    Some((fstype, options))
}

/// Checks whether `location` (an existing directory, e.g., /run/media/astronaut/DISQUE)
/// can hold the image file. `None` = usable
pub fn check_destination(location: &Path) -> Option<DestinationProblem> {
    if !location.is_dir() {
        return Some(DestinationProblem::Missing);
    }
    let (fstype, options) = match filesystem_info(location) {
        Some(v) => v,
        None => return None, // findmnt is unavailable: the helper will check again
    };
    match fstype.as_str() {
        "vfat" | "msdos" | "fat" => return Some(DestinationProblem::Fat32),
        "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "fuse.sshfs" | "9p" => {
            return Some(DestinationProblem::Network)
        }
        "tmpfs" | "ramfs" | "devtmpfs" => return Some(DestinationProblem::Temporary),
        _ => {}
    }
    if options.split(',').any(|o| o == "ro") {
        return Some(DestinationProblem::ReadOnly);
    }
    None
}

/// Image file size for `required` bytes of data: +20% +2 GiB
/// (allowance for snapshots and Btrfs metadata), minimum 4 GiB,
/// rounded to the nearest MiB. Calculated WITHOUT accounting for compression (zstd): conservative estimate
pub fn image_size_for(required: u64) -> u64 {
    let with_margin = required
        .saturating_add(required / 5)
        .saturating_add(2 * GIB);
    let size = with_margin.max(4 * GIB);
    ((size + MIB - 1) / MIB) * MIB
}

/// Free space (in bytes) on the file system containing `path` (which must exist)
pub fn available_space(path: &Path) -> Option<u64> {
    let out = Command::new("df")
        .args(["--output=avail", "-B1"])
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().nth(1)?.trim().parse::<u64>().ok()
}

/// Result of evaluating a location for a given amount of data
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    pub problem: Option<DestinationProblem>,
    /// Image size for this data
    pub needed: u64,
    /// Available space: free + image already present
    pub available: u64,
    pub existing: u64,
    pub enough: bool,
}

/// Allocates `data_size` bytes of data in `dest` (the folder that will contain the image, even if it does not
/// yet exist)
pub fn evaluate(dest: &Path, data_size: u64) -> Evaluation {
    let probe = existing_ancestor(dest);
    let needed = image_size_for(data_size);
    let existing = existing_image_size(dest).unwrap_or(0);
    let problem = check_destination(&probe);
    let free = available_space(&probe).unwrap_or(0);
    let available = free.saturating_add(existing);
    // Image already created: the helper only enlarges it if necessary, so
    // it's enough that the space isn't full
    let enough = problem.is_none()
        && if existing > 0 {
            free >= HOST_MARGIN
        } else {
            available >= needed.saturating_add(HOST_MARGIN)
        };
    Evaluation {
        problem,
        needed,
        available,
        existing,
        enough,
    }
}

/// Size of the image already present in `dest` (folder .../backup), if it exists
pub fn existing_image_size(dest: &Path) -> Option<u64> {
    std::fs::metadata(dest.join(IMAGE_FILE_NAME))
        .ok()
        .map(|m| m.len())
}

/// Locates the root helper. Order: ASTROARCH_BACKUP_HELPER variable, installed
/// path, then the “scripts/” directory next to the executable (development build)
pub fn helper_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ASTROARCH_BACKUP_HELPER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let installed = PathBuf::from(INSTALLED_HELPER);
    if installed.is_file() {
        return Some(installed);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in ["scripts", "../scripts", "../../scripts"] {
                let candidate = dir.join(rel).join(DEV_HELPER_NAME);
                if candidate.is_file() {
                    // pkexec requires an absolute path
                    return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_size_has_minimum() {
        assert_eq!(image_size_for(0), 4 * GIB);
        assert_eq!(image_size_for(GIB), 4 * GIB);
    }

    #[test]
    fn image_size_adds_margin_and_rounds() {
        let size = image_size_for(15 * GIB);
        assert!(size >= 15 * GIB + 3 * GIB + 2 * GIB);
        assert_eq!(size % MIB, 0);
    }

    #[test]
    fn evaluate_missing_destination_uses_ancestor() {
        let ev = evaluate(Path::new("/tmp/does/not/exist/backup"), 1024);
        assert_eq!(ev.existing, 0);
        assert!(ev.needed >= 4 * GIB);
    }

    #[test]
    fn protected_paths() {
        let home = "/home/astronaut";
        assert!(is_protected_path(home, "/home/astronaut/Desktop"));
        assert!(is_protected_path(home, "/home/astronaut/.config"));
        assert!(is_protected_path(home, "/home/astronaut/.local/share/x"));
        assert!(is_protected_path(home, "/home/astronaut/.oh-my-zsh"));
        assert!(is_protected_path(home, "/home/astronaut/.astroarch/assets"));
        assert!(!is_protected_path(home, "/home/astronaut/Documents"));
        assert!(!is_protected_path(home, "/home/astronaut/Desktopx"));
        assert!(!is_protected_path(home, "/home/astronaut/.cache"));
        assert!(!is_protected_path("", "/Desktop"));
    }

    #[test]
    fn problem_codes() {
        assert_eq!(DestinationProblem::Fat32.code(), "fat32");
        assert_eq!(DestinationProblem::Missing.code(), "missing");
    }

    #[test]
    fn ancestor_of_missing_path_exists() {
        let p = existing_ancestor(Path::new("/tmp/does/not/exist/backup"));
        assert!(p.is_dir());
    }
}

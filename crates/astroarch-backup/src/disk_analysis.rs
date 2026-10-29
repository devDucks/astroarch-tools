#![allow(dead_code)]

use std::path::Path;
use std::process::Command;
use thiserror::Error;

use crate::storage;

#[derive(Error, Debug)]
pub enum DiskError {
    #[error("{0}")]
    HelperFailed(String),

    #[error("Failed to parse disk size")]
    ParseError,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid path")]
    InvalidPath,
}

#[derive(Debug, Clone)]
pub struct DiskAnalysis {
    pub total_to_backup: u64,
    pub available_space: u64,
    pub has_enough_space: bool,
}

impl DiskAnalysis {
    pub fn human_readable(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_idx = 0;

        while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size /= 1024.0;
            unit_idx += 1;
        }

        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

/// Determining what will be backed up. The list of system exclusions is stored
/// in ONE place: the root helper (scripts/astroarch-backup-helper.sh),
/// which is used for both determining what to back up and actually performing the backup. Here, we only keep the
/// folders excluded by the user
pub struct DiskAnalyzer {
    excluded_folders: Vec<String>,
}

impl Default for DiskAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskAnalyzer {
    pub fn new() -> Self {
        Self {
            excluded_folders: Vec::new(),
        }
    }

    /// Name of a folder in the personal directory (e.g., “Documents”)
    pub fn add_exclusion(&mut self, name: &str) {
        self.excluded_folders.push(name.to_string());
    }

    /// Bytes to be copied, measured by a dummy rsync command run by the helper.
    /// `dest` (the .../backup directory) is passed in to be excluded from the measurement
    pub fn calculate_backup_size(&self, dest: &Path) -> Result<u64, DiskError> {
        let helper = storage::helper_path().ok_or_else(|| {
            DiskError::HelperFailed(
                "Helper script not found (astroarch-backup-helper)".to_string(),
            )
        })?;

        let mut cmd = Command::new("pkexec");
        cmd.arg(&helper).arg("measure").arg("--dest").arg(dest);
        for folder in &self.excluded_folders {
            cmd.arg("--exclude").arg(folder);
        }

        eprintln!(
            "Running: pkexec {} measure --dest {} (excluding: {:?})",
            helper.display(),
            dest.display(),
            self.excluded_folders
        );
        let output = cmd
            .output()
            .map_err(|e| DiskError::HelperFailed(format!("Cannot start pkexec: {}", e)))?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        if !output.status.success() {
            return Err(match output.status.code() {
                Some(126) | Some(127) => DiskError::HelperFailed(
                    "Authorization was refused or cancelled".to_string(),
                ),
                code => DiskError::HelperFailed(
                    Self::parse_helper_error(&stdout)
                        .unwrap_or_else(|| format!("The measure failed (exit code {:?})", code)),
                ),
            });
        }

        Self::parse_helper_size(&stdout)
    }

    /// Search for “RESULT:size:<bytes>” in the helper's output
    fn parse_helper_size(output: &str) -> Result<u64, DiskError> {
        for line in output.lines() {
            if let Some(value) = line.trim().strip_prefix("RESULT:size:") {
                return value.trim().parse::<u64>().map_err(|_| DiskError::ParseError);
            }
        }
        Err(DiskError::ParseError)
    }

    /// Searches for “ERROR:<code>:<message>” and returns the message
    fn parse_helper_error(output: &str) -> Option<String> {
        for line in output.lines() {
            if let Some(rest) = line.trim().strip_prefix("ERROR:") {
                let msg = rest.splitn(2, ':').nth(1).unwrap_or(rest);
                return Some(msg.to_string());
            }
        }
        None
    }

    /// Free space on the file system containing `path` (which must exist)
    pub fn get_available_space(path: &Path) -> Result<u64, DiskError> {
        let output = Command::new("df")
            .args(["--output=avail", "-B1"])
            .arg(path)
            .output()
            .map_err(|e| DiskError::HelperFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(DiskError::HelperFailed("df command failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        if lines.len() < 2 {
            return Err(DiskError::ParseError);
        }

        let available = lines[1]
            .trim()
            .parse::<u64>()
            .map_err(|_| DiskError::ParseError)?;
        eprintln!("Available space: {} bytes", available);
        Ok(available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_readable() {
        assert_eq!(DiskAnalysis::human_readable(512), "512.00 B");
        assert_eq!(DiskAnalysis::human_readable(1024 * 1024), "1.00 MB");
        assert_eq!(DiskAnalysis::human_readable(5 * 1024 * 1024 * 1024), "5.00 GB");
    }

    #[test]
    fn test_parse_helper_size() {
        let out = "STAGE:measuring\nRESULT:size:12345678\n";
        assert_eq!(DiskAnalyzer::parse_helper_size(out).unwrap(), 12345678);
    }

    #[test]
    fn test_parse_helper_size_missing() {
        assert!(DiskAnalyzer::parse_helper_size("nothing here").is_err());
    }

    #[test]
    fn test_parse_helper_error() {
        let out = "ERROR:12:FAT32 drives cannot be used\n";
        assert_eq!(
            DiskAnalyzer::parse_helper_error(out).unwrap(),
            "FAT32 drives cannot be used"
        );
    }
}

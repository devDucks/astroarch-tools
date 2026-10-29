#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Invalid destination path")]
    InvalidPath,
    
    #[error("Config file corrupted")]
    Corrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupConfig {
    pub dest_path: PathBuf,
    pub first_backup: bool,
    pub auto_choice: AutoChoice,
    /// Number of snapshots retained; configurable in the interface
    pub keep_snapshots: u32,
    /// Most recently excluded folders (absolute paths): re-included and used
    /// for the backup prior to a restore
    pub excluded_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AutoChoice {
    Ask,
    Yes,
    Never,
}

impl Default for AutoChoice {
    fn default() -> Self {
        AutoChoice::Ask
    }
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            // Last location used; by default, the default folder on the primary disk
            dest_path: PathBuf::from("/var/backups"),
            first_backup: true,
            auto_choice: AutoChoice::Ask,
            keep_snapshots: 5,
            excluded_paths: Vec::new(),
        }
    }
}

impl BackupConfig {
    /// Retention limited to 1..=50
    pub fn keep_clamped(&self) -> u32 {
        self.keep_snapshots.clamp(1, 50)
    }
}

pub struct ConfigManager;

impl ConfigManager {
    pub fn config_path() -> Result<PathBuf, ConfigError> {
        let home = std::env::var("HOME")
            .map_err(|_| ConfigError::InvalidPath)?;
        Ok(PathBuf::from(home).join(".backup_dest"))
    }

    pub fn load() -> Result<BackupConfig, ConfigError> {
        let path = Self::config_path()?;
        
        if !path.exists() {
            return Ok(BackupConfig::default());
        }

        let content = fs::read_to_string(&path)?;
        
        // Parse as JSON (new format)
        if let Ok(config) = serde_json::from_str::<BackupConfig>(&content) {
            return Ok(config);
        }

        // Fallback: parse as shell key=value format
        Self::parse_shell_format(&content)
    }

    pub fn save(config: &BackupConfig) -> Result<(), ConfigError> {
        let path = Self::config_path()?;
        let content = serde_json::to_string_pretty(config)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn parse_shell_format(content: &str) -> Result<BackupConfig, ConfigError> {
        let mut config = BackupConfig::default();

        for line in content.lines() {
            if let Some(idx) = line.find('=') {
                let (key, value) = line.split_at(idx);
                let value = value.trim_start_matches('=').trim_matches('"');

                match key {
                    "DEST_PATH" => {
                        config.dest_path = PathBuf::from(value);
                    }
                    "FIRST_BACKUP" => {
                        config.first_backup = value.eq_ignore_ascii_case("yes");
                    }
                    "AUTO_CHOICE" => {
                        config.auto_choice = match value {
                            "Yes" => AutoChoice::Yes,
                            "Never" => AutoChoice::Never,
                            _ => AutoChoice::Ask,
                        };
                    }
                    _ => {}
                }
            }
        }

        Ok(config)
    }

    pub fn update_choice(choice: AutoChoice) -> Result<(), ConfigError> {
        let mut config = Self::load()?;
        config.auto_choice = choice;
        Self::save(&config)
    }

    pub fn mark_backup_complete() -> Result<(), ConfigError> {
        let mut config = Self::load()?;
        config.first_backup = false;
        Self::save(&config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shell_format() {
        let shell_content = r#"
DEST_PATH="/mnt/backup"
FIRST_BACKUP="yes"
AUTO_CHOICE="Ask"
"#;
        let config = ConfigManager::parse_shell_format(shell_content).unwrap();
        assert_eq!(config.dest_path, PathBuf::from("/mnt/backup"));
        assert!(config.first_backup);
        assert_eq!(config.auto_choice, AutoChoice::Ask);
    }

    #[test]
    fn test_json_format() {
        let json_content = r#"{
  "dest_path": "/mnt/backup",
  "first_backup": true,
  "auto_choice": "ask"
}"#;
        let config: BackupConfig = serde_json::from_str(json_content).unwrap();
        assert_eq!(config.dest_path, PathBuf::from("/mnt/backup"));
        // Field missing from an older file: default value
        assert_eq!(config.keep_snapshots, 5);
    }

    #[test]
    fn test_keep_clamped() {
        let mut config = BackupConfig::default();
        config.keep_snapshots = 0;
        assert_eq!(config.keep_clamped(), 1);
        config.keep_snapshots = 999;
        assert_eq!(config.keep_clamped(), 50);
    }
}

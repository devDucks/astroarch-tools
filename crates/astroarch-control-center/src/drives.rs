//! The drives the pages offer (Image storage, Data backup): the backup tool's list
//! (media.rs) without the partitions of another system.

use serde_json::Value;

use crate::media;

/// ALARM_BOOT / ALARM_ROOT: the disk of another AstroArch (or Arch Linux ARM), never offered
pub fn offered(drive: &Value) -> bool {
    !drive["label"].as_str().unwrap_or("").starts_with("ALARM_")
}

/// The backup tool's drives for `data_size` bytes of data, without those of another system
pub fn list(data_size: u64) -> Vec<Value> {
    media::list(data_size).into_iter().filter(offered).collect()
}

#[cfg(test)]
mod tests {
    use super::offered;
    use serde_json::json;

    #[test]
    fn another_system_is_not_offered() {
        assert!(!offered(&json!({ "label": "ALARM_ROOT" })));
        assert!(!offered(&json!({ "label": "ALARM_BOOT" })));
        assert!(offered(&json!({ "label": "Astro SSD" })));
        assert!(offered(&json!({ "label": "/dev/sda1" })));
    }
}

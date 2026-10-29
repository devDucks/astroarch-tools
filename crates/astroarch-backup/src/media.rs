//! Backup media detection: all file systems that are
//! NOT on the system disk (the one containing / and /boot).
//!
//! Read-only (lsblk, findmnt, df); mounting unmounted media is done
//! via udisksctl (without root privileges, via polkit).

use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::storage::{self, DestinationProblem};

#[derive(Debug, Clone)]
struct Node {
    path: String,
    size: u64,
    fstype: String,
    label: String,
    mountpoints: Vec<String>,
    typ: String,
    /// Name of the root disk of the lsblk tree to which this node belongs
    disk: String,
}

const SKIP_FS: &[&str] = &[
    "swap",
    "LVM2_member",
    "crypto_LUKS",
    "linux_raid_member",
    "squashfs",
    "iso9660",
    "zfs_member",
];

fn as_u64(v: &Value) -> u64 {
    match v {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn as_string(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn collect(node: &Value, disk: &str, out: &mut Vec<Node>) {
    let name = as_string(&node["name"]);
    let disk_name = if disk.is_empty() {
        name
    } else {
        disk.to_string()
    };
    let mountpoints: Vec<String> = node["mountpoints"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();
    out.push(Node {
        path: as_string(&node["path"]),
        size: as_u64(&node["size"]),
        fstype: as_string(&node["fstype"]),
        label: as_string(&node["label"]),
        mountpoints,
        typ: as_string(&node["type"]),
        disk: disk_name.clone(),
    });
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect(child, &disk_name, out);
        }
    }
}

fn parse_nodes(text: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    if let Ok(root) = serde_json::from_str::<Value>(text) {
        if let Some(devices) = root["blockdevices"].as_array() {
            for device in devices {
                collect(device, "", &mut nodes);
            }
        }
    }
    nodes
}

fn lsblk_nodes() -> Vec<Node> {
    let out = Command::new("lsblk")
        .args(["-J", "-b", "-o", "NAME,PATH,SIZE,FSTYPE,LABEL,MOUNTPOINTS,TYPE"])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_nodes(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Device that contains `mount` (e.g., /dev/sda2), without the suffix [/@subvol]
fn source_of(mount: &str) -> Option<String> {
    let out = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", mount])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let source = text.split('[').next().unwrap_or("").to_string();
    if source.is_empty() {
        None
    } else {
        Some(source)
    }
}

/// System disks: those containing /, /boot, or /boot/firmware
fn system_disks(nodes: &[Node]) -> HashSet<String> {
    let mut disks = HashSet::new();
    for mount in ["/", "/boot", "/boot/firmware"] {
        if let Some(source) = source_of(mount) {
            for n in nodes {
                if n.path == source {
                    disks.insert(n.disk.clone());
                }
            }
        }
    }
    disks
}
/// (status, available space, required image size, destination folder)
fn classify(n: &Node, mount: &str, data_size: u64) -> (String, u64, u64, String) {
    if matches!(n.fstype.as_str(), "vfat" | "msdos" | "fat") {
        return ("fat32".to_string(), 0, 0, String::new());
    }
    if mount.is_empty() {
        return ("not_mounted".to_string(), 0, 0, String::new());
    }
    let dest = Path::new(mount).join(storage::MEDIA_SUBDIR);
    let ev = storage::evaluate(&dest, data_size);
    let state = match ev.problem {
        Some(DestinationProblem::Fat32) => "fat32",
        Some(DestinationProblem::ReadOnly) => "readonly",
        Some(_) => "unsupported",
        None => {
            if ev.enough {
                "ready"
            } else {
                "too_small"
            }
        }
    };
    (
        state.to_string(),
        ev.available,
        ev.needed,
        dest.to_string_lossy().to_string(),
    )
}

fn build(nodes: &[Node], system: &HashSet<String>, data_size: u64) -> Vec<Value> {
    let mut result = Vec::new();
    for n in nodes {
        if n.typ != "part" && n.typ != "disk" {
            continue;
        }
        if n.fstype.is_empty() || SKIP_FS.contains(&n.fstype.as_str()) {
            continue;
        }
        if system.contains(&n.disk) {
            continue;
        }
        let mount = n
            .mountpoints
            .iter()
            .find(|m| !m.is_empty())
            .cloned()
            .unwrap_or_default();
        // Belt and suspenders: never a system partition
        if mount == "/" || mount.starts_with("/boot") {
            continue;
        }
        let (state, available, needed, destination) = classify(n, &mount, data_size);
        // Is there already an AstroArch backup on this storage medium?
        let has_backup = !destination.is_empty()
            && Path::new(&destination)
                .join(storage::IMAGE_FILE_NAME)
                .exists();
        let label = if n.label.is_empty() {
            n.path.clone()
        } else {
            n.label.clone()
        };
        result.push(json!({
            "device": n.path,
            "label": label,
            "fstype": n.fstype,
            "size": n.size,
            "mountpoint": mount,
            "available": available,
            "needed": needed,
            "state": state,
            "destination": destination,
            "hasBackup": has_backup,
        }));
    }
    result
}

/// Candidate media, with their status relative to `data_size` bytes of data
pub fn list(data_size: u64) -> Vec<Value> {
    let nodes = lsblk_nodes();
    let system = system_disks(&nodes);
    build(&nodes, &system, data_size)
}

fn parse_mount_output(text: &str) -> Option<String> {
    // « /dev/sdb a été monté sur /run/media/astronaut/LABEL. »
    // « Erreur lors du montage de /dev/sdb : ... est déjà monté sur `/run/media/astronaut/LABEL'. »
    let line = text.lines().find(|l| l.contains(" at "))?;
    let idx = line.rfind(" at ")?;
    let rest = &line[idx + 4..];
    let path = rest.trim_matches(|c: char| c.is_whitespace() || c == '.' || c == '`' || c == '\'' || c == '"');
    if path.starts_with('/') {
        Some(path.to_string())
    } else {
        None
    }
}

/// Mounts a device using udisks (without root privileges) and returns its mount point
pub fn mount(device: &str) -> Result<String, String> {
    let valid = device.starts_with("/dev/")
        && device
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'));
    if !valid {
        return Err("Invalid device".to_string());
    }
    let out = Command::new("udisksctl")
        .args(["mount", "-b", device])
        .output()
        .map_err(|e| format!("udisksctl is not available: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let text = format!("{}\n{}", stdout, stderr);
    match parse_mount_output(&text) {
        Some(mountpoint) => Ok(mountpoint),
        None => Err(text.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"blockdevices":[
      {"name":"sda","path":"/dev/sda","size":500107862016,"fstype":null,"label":null,"mountpoints":[null],"type":"disk","children":[
        {"name":"sda1","path":"/dev/sda1","size":805306368,"fstype":"vfat","label":"ALARM_BOOT","mountpoints":["/boot"],"type":"part"},
        {"name":"sda2","path":"/dev/sda2","size":499000000000,"fstype":"ext4","label":"ALARM_ROOT","mountpoints":["/"],"type":"part"}]},
      {"name":"sdb","path":"/dev/sdb","size":"16000000000","fstype":"ext4","label":"SAVE","mountpoints":[null],"type":"disk"}]}"#;

    #[test]
    fn parses_tree_and_disk_names() {
        let nodes = parse_nodes(SAMPLE);
        assert_eq!(nodes.len(), 4);
        let sda2 = nodes.iter().find(|n| n.path == "/dev/sda2").unwrap();
        assert_eq!(sda2.disk, "sda");
        let sdb = nodes.iter().find(|n| n.path == "/dev/sdb").unwrap();
        assert_eq!(sdb.size, 16_000_000_000);
    }

    #[test]
    fn system_disk_is_excluded() {
        let nodes = parse_nodes(SAMPLE);
        let mut system = HashSet::new();
        system.insert("sda".to_string());
        let media = build(&nodes, &system, 1024);
        assert_eq!(media.len(), 1);
        assert_eq!(media[0]["device"], "/dev/sdb");
        assert_eq!(media[0]["state"], "not_mounted");
    }

    #[test]
    fn mount_output_variants() {
        assert_eq!(
            parse_mount_output("Mounted /dev/sdb at /run/media/astronaut/SAVE."),
            Some("/run/media/astronaut/SAVE".to_string())
        );
        assert_eq!(
            parse_mount_output("Error: Object /dev/sdb is already mounted at `/run/media/astronaut/My Disk'."),
            Some("/run/media/astronaut/My Disk".to_string())
        );
        assert_eq!(parse_mount_output("Not authorized"), None);
    }
}

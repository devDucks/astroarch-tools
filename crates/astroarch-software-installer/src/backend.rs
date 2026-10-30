//! Logique sans Qt : lecture du fichier TOML, interrogation de pacman,
//! installation / désinstallation.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

/// Locations checked (in order) if neither an argument nor an environment variable is provided
const DEFAULT_PATHS: &[&str] = &[
    "/etc/astroarch/software.toml",
    "/usr/share/astroarch-software-installer/software.toml",
    "data/software.toml",
    "/home/astronaut/.astroarch/configs/software.toml"
];

#[derive(Debug, Deserialize)]
struct Catalog {
    #[serde(default)]
    package: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    ///Pacman package name (required)
    name: String,
    /// Displayed title (optional; otherwise, `name`)
    title: Option<String>,
    /// Fallback description, in English (used if the repository does not provide one)
    description: Option<String>,
    /// Catégorie (facultatif)
    category: Option<String>,
    /// `true`: AUR package, installed with `paru` (optional)
    #[serde(default)]
    aur: bool,
}

/// Line sent to QML (serialized as JSON).
#[derive(Debug, Serialize)]
struct Item {
    name: String,
    title: String,
    description: String,
    category: String,
    installed: bool,
    available: bool,
    aur: bool,
}

pub struct Outcome {
    pub ok: bool,
    pub installed: bool,
    /// "" | "auth" | "other"
    pub kind: String,
    pub detail: String,
}

/// Accepted package names
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-'))
}

/// 1. first command-line argument, 2. $ASTROARCH_SOFTWARE_LIST, 3. default locations
fn catalog_path() -> Option<PathBuf> {
    if let Some(arg) = std::env::args().nth(1) {
        if !arg.starts_with('-') {
            return Some(PathBuf::from(arg));
        }
    }
    if let Ok(p) = std::env::var("ASTROARCH_SOFTWARE_LIST") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    DEFAULT_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        // Development build, started from anywhere: this crate's list. The Control Center
        // compiles this file too, hence the path through the crates folder
        .or_else(|| {
            let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../astroarch-software-installer/data/software.toml");
            p.is_file().then_some(p)
        })
}

fn pacman_stdout(args: &[&str]) -> Result<String, String> {
    let out = Command::new("pacman")
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("cannot run pacman: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Installed packages
fn installed_set() -> Result<HashSet<String>, String> {
    Ok(pacman_stdout(&["-Qq"])?
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Packages present in the synchronization repositories
/// Line format: `repo name version [installed]`
fn sync_names() -> HashSet<String> {
    pacman_stdout(&["-Sl"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .collect()
}

/// Repository descriptions
fn repo_descriptions(names: &[String]) -> HashMap<String, String> {
    if names.is_empty() {
        return HashMap::new();
    }
    let mut args: Vec<&str> = vec!["-Si", "--"];
    args.extend(names.iter().map(String::as_str));
    parse_si(&pacman_stdout(&args).unwrap_or_default())
}

fn parse_si(out: &str) -> HashMap<String, String> {
    fn flush(map: &mut HashMap<String, String>, name: &mut Option<String>, desc: &mut String) {
        if let Some(n) = name.take() {
            let d = desc.trim();
            if !d.is_empty() {
                map.insert(n, d.to_string());
            }
        }
        desc.clear();
    }

    let mut map = HashMap::new();
    let mut name: Option<String> = None;
    let mut desc = String::new();
    let mut in_desc = false;

    for line in out.lines() {
        if line.trim().is_empty() {
            flush(&mut map, &mut name, &mut desc);
            in_desc = false;
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            if in_desc {
                desc.push(' ');
                desc.push_str(line.trim());
            }
            continue;
        }
        in_desc = false;
        if let Some((key, value)) = line.split_once(':') {
            match key.trim() {
                "Name" => {
                    flush(&mut map, &mut name, &mut desc);
                    name = Some(value.trim().to_string());
                }
                "Description" => {
                    desc = value.trim().to_string();
                    in_desc = true;
                }
                _ => {}
            }
        }
    }
    flush(&mut map, &mut name, &mut desc);
    map
}

/// Is `paru` installed? (AUR Assistant)
fn paru_available() -> bool {
    Command::new("paru")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Reads the TOML, queries Pacman, and returns the list as JSON
pub fn load_items() -> Result<String, String> {
    let path = catalog_path()
        .ok_or_else(|| "no software list found (set ASTROARCH_SOFTWARE_LIST)".to_string())?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let catalog: Catalog =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let entries: Vec<Entry> = catalog
        .package
        .into_iter()
        .filter(|e| {
            let ok = valid_name(&e.name);
            if !ok {
                eprintln!("software list: ignoring invalid package name {:?}", e.name);
            }
            ok
        })
        .collect();

    let installed = installed_set()?;
    let sync = sync_names();
    // If the synchronization bases are unreadable, nothing is blocked
    let sync_known = !sync.is_empty();
    let paru_ok = paru_available();

    let to_describe: Vec<String> = entries
        .iter()
        .filter(|e| sync.contains(&e.name))
        .map(|e| e.name.clone())
        .collect();
    let repo_desc = repo_descriptions(&to_describe);

    let items: Vec<Item> = entries
        .into_iter()
        .map(|e| {
            // Description: Repository first; otherwise, TOML file
            let description = repo_desc
                .get(&e.name)
                .cloned()
                .or(e.description)
                .unwrap_or_default();
            // AUR package: can only be installed if “paru” is present.
            // Repository package: can be installed if it is in the synchronization databases
            let available = if e.aur {
                paru_ok
            } else {
                !sync_known || sync.contains(&e.name)
            };
            Item {
                title: e.title.unwrap_or_else(|| e.name.clone()),
                description,
                category: e.category.unwrap_or_default(),
                installed: installed.contains(&e.name),
                available,
                aur: e.aur,
                name: e.name,
            }
        })
        .collect();

    serde_json::to_string(&items).map_err(|e| e.to_string())
}

fn is_installed(name: &str) -> bool {
    Command::new("pacman")
        .args(["-Q", "--", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn last_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// pkexec messages when the user closes the window or does not have permission
/// (since it returns an exit code of 1, we also check the text)
fn is_auth_failure(code: Option<i32>, text: &str) -> bool {
    if matches!(code, Some(126) | Some(127)) {
        return true;
    }
    let t = text.to_lowercase();
    t.contains("request dismissed") || t.contains("not authorized")
}

/// Installs or uninstalls a package.
/// - repository: `pkexec pacman -S` (polkit authentication window);
/// - AUR (`aur = true`): `paru -S`, run with user privileges (makepkg does not accept
///   root) and `--sudo pkexec` for steps that require root;
/// - uninstallation: always `pkexec pacman -Rns` (also applies to AUR packages)
pub fn change(name: &str, install: bool, aur: bool) -> Outcome {
    if !valid_name(name) {
        return Outcome {
            ok: false,
            installed: false,
            kind: "other".into(),
            detail: format!("invalid package name: {name}"),
        };
    }

    let mut cmd;
    if install && aur {
        cmd = Command::new("paru");
        // --skipreview: no interactive review of the PKGBUILD (no terminal);
        // --nosudoloop: no periodic invocation of pkexec during compilation
        cmd.args([
            "-S",
            "--needed",
            "--noconfirm",
            "--skipreview",
            "--nosudoloop",
            "--sudo",
            "pkexec",
            name,
        ]);
    } else {
        cmd = Command::new("pkexec");
        cmd.arg("pacman");
        if install {
            cmd.args(["-S", "--needed", "--noconfirm", "--", name]);
        } else {
            cmd.args(["-Rns", "--noconfirm", "--", name]);
        }
    }

    match cmd.output() {
        Err(e) => Outcome {
            ok: false,
            installed: is_installed(name),
            kind: "other".into(),
            detail: format!("cannot run command: {e}"),
        },
        Ok(out) => {
            let ok = out.status.success();
            let code = out.status.code();
            let all_text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            );
            let kind = if ok {
                ""
            } else if is_auth_failure(code, &all_text) {
                "auth"
            } else {
                "other"
            };
            let mut detail = last_line(&out.stderr);
            if detail.is_empty() {
                detail = last_line(&out.stdout);
            }
            Outcome {
                ok,
                installed: is_installed(name),
                kind: kind.into(),
                detail: if ok { String::new() } else { detail },
            }
        }
    }
}

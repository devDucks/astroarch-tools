//! Explore available keyboard layouts via xkeyboard-config
//! and apply them in real time to the test area.
//!
//! Ref: https://wiki.archlinux.org/title/Linux_console/Keyboard_configuration
//! (console) — Under Plasma/X11/Wayland, configuration is done via
//! `localectl set-x11-keymap` (+ kxkbrc under Wayland); the console configuration (vconsole.conf) is
//! generated separately using `localectl set-keymap` if necessary

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct KeyboardLayout {
    pub code: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct KeyboardVariant {
    pub code: String,
    pub description: String,
}

const XKB_BASE_LST: &str = "/usr/share/X11/xkb/rules/base.lst";

/// Parses the “! layout” section of base.lst
pub fn list_layouts() -> std::io::Result<Vec<KeyboardLayout>> {
    parse_section(XKB_BASE_LST, "! layout").map(|rows| {
        rows.into_iter()
            .map(|(code, description)| KeyboardLayout { code, description })
            .collect()
    })
}

/// Parses the “! variant” section of base.lst, filtered by a given layout.
/// The line format is: “variant  layout: description”
pub fn list_variants(layout: &str) -> std::io::Result<Vec<KeyboardVariant>> {
    let content = std::fs::read_to_string(XKB_BASE_LST)?;
    let mut in_section = false;
    let mut variants = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('!') {
            in_section = line == "! variant";
            continue;
        }
        if !in_section || line.is_empty() {
            continue;
        }
        // format: "code  layout1,layout2: Description text"
        if let Some((code, rest)) = line.split_once(char::is_whitespace) {
            let rest = rest.trim();
            if let Some((layouts, desc)) = rest.split_once(':') {
                let layouts: Vec<&str> = layouts.split(',').map(str::trim).collect();
                if layouts.contains(&layout) {
                    variants.push(KeyboardVariant {
                        code: code.to_string(),
                        description: desc.trim().to_string(),
                    });
                }
            }
        }
    }

    Ok(variants)
}

fn parse_section(path: &str, section_header: &str) -> std::io::Result<Vec<(String, String)>> {
    let content = std::fs::read_to_string(path)?;
    let mut in_section = false;
    let mut out = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('!') {
            in_section = line == section_header;
            continue;
        }
        if !in_section || line.is_empty() {
            continue;
        }
        if let Some((code, desc)) = line.split_once(char::is_whitespace) {
            out.push((code.to_string(), desc.trim().to_string()));
        }
    }

    Ok(out)
}

/// Mapping table: country code (ISO 3166-1 alpha-2) → code
/// Default XKB layout. Covers common cases; otherwise, it
/// falls back to “us”.
pub fn default_layout_for_country(country_code: &str) -> &'static str {
    let map: HashMap<&str, &str> = HashMap::from([
        ("FR", "fr"), ("BE", "be"), ("CH", "ch"), ("DE", "de"),
        ("AT", "at"), ("IT", "it"), ("ES", "es"), ("PT", "pt"),
        ("GB", "gb"), ("US", "us"), ("CA", "ca"), ("NL", "nl"),
        ("PL", "pl"), ("CZ", "cz"), ("SK", "sk"), ("HU", "hu"),
        ("RO", "ro"), ("BG", "bg"), ("GR", "gr"), ("SE", "se"),
        ("NO", "no"), ("DK", "dk"), ("FI", "fi"), ("IS", "is"),
        ("IE", "ie"), ("RU", "ru"), ("UA", "ua"), ("TR", "tr"),
        ("JP", "jp"), ("KR", "kr"), ("CN", "cn"), ("BR", "br"),
        ("MX", "latam"), ("AR", "latam"), ("CL", "latam"),
        ("MA", "ma"), ("DZ", "dz"), ("TN", "tn"), ("IL", "il"),
        ("SA", "sa"), ("IN", "in"), ("ZA", "za"), ("AU", "au"),
        ("NZ", "us"), ("LU", "lu"), ("SI", "si"), ("HR", "hr"),
        ("RS", "rs"), ("EE", "ee"), ("LV", "lv"), ("LT", "lt"),
    ]);
    map.get(country_code).copied().unwrap_or("us")
}

/// `true` in a Wayland session (Plasma/KWin). There, `setxkbmap` only reaches the
/// Xwayland server (hence "Running setxkbmap against an Xwayland server") and changes
/// neither the Wayland session nor the application's own test area: the layout has to be
/// handed to KWin through `kxkbrc` + the `org.kde.keyboard` D-Bus service.
pub fn is_wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|t| t.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

fn other(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, msg.into())
}

// ---------------------------------------------------------------------------
// KDE Plasma / Wayland : kxkbrc + D-Bus
// ---------------------------------------------------------------------------

/// Values of the `[Layout]` group of ~/.config/kxkbrc, saved before the first live change
/// so that they can be restored if the user quits without applying.
#[derive(Debug, Clone)]
struct KxkbSnapshot {
    use_flag: String,
    layout_list: String,
    variant_list: String,
}

static LIVE_BACKUP: Mutex<Option<KxkbSnapshot>> = Mutex::new(None);
static PERSISTED: AtomicBool = AtomicBool::new(false);

fn kxkb_read(key: &str) -> String {
    Command::new("kreadconfig6")
        .args(["--file", "kxkbrc", "--group", "Layout", "--key", key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Writes one key of the `[Layout]` group of kxkbrc.
/// `--notify` is essential: KWin watches kxkbrc through a KConfigWatcher, which reacts to the
/// D-Bus change notification and not to the file write itself. Without it nothing happens
/// until the next login. (If this kwriteconfig6 does not know `--notify`, retry without.)
fn kxkb_write(key: &str, value: &str) -> std::io::Result<()> {
    let run = |notify: bool| {
        let mut cmd = Command::new("kwriteconfig6");
        if notify {
            cmd.arg("--notify");
        }
        cmd.args(["--file", "kxkbrc", "--group", "Layout", "--key", key, value])
            .status()
    };
    let status = run(true)
        .map_err(|e| other(format!("kwriteconfig6 introuvable (paquet kconfig) : {e}")))?;
    if status.success() {
        return Ok(());
    }
    match run(false) {
        Ok(s) if s.success() => Ok(()),
        _ => Err(other(format!("kwriteconfig6 a échoué pour la clé {key}"))),
    }
}

/// Tells KWin / the keyboard daemon to re-read kxkbrc (same signal as the
/// "Keyboard" System Settings page).
fn kxkb_reload() -> std::io::Result<()> {
    let status = Command::new("dbus-send")
        .args([
            "--session",
            "--type=signal",
            "--reply-timeout=100",
            "/Layouts",
            "org.kde.keyboard.reloadConfig",
        ])
        .status()
        .map_err(|e| other(format!("dbus-send introuvable : {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(other("dbus-send org.kde.keyboard.reloadConfig a échoué"))
    }
}

/// Calls a method of KWin's keyboard service (`org.kde.keyboard`, object `/Layouts`) and
/// returns the strings of its reply (empty if the service does not answer).
fn kwin_keyboard_call(method: &str, args: &[&str]) -> Option<Vec<String>> {
    let out = Command::new("dbus-send")
        .args(["--session", "--print-reply", "--reply-timeout=500", "--dest=org.kde.keyboard", "/Layouts", method])
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    Some(
        text.lines()
            .filter_map(|l| {
                let l = l.trim();
                l.strip_prefix("string \"")
                    .and_then(|r| r.strip_suffix('"'))
                    .map(str::to_string)
                    .or_else(|| l.strip_prefix("uint32 ").map(|n| format!("#{n}")))
            })
            .collect(),
    )
}

/// Makes `layout` (+ `variant`) the single active layout of the Plasma session.
fn kxkb_apply(layout: &str, variant: Option<&str>) -> std::io::Result<()> {
    // Every write notifies KWin, which re-reads kxkbrc each time: the order below keeps every
    // intermediate state valid (never "new layout + old variant").
    kxkb_write("Use", "true")?;
    kxkb_write("VariantList", "")?;
    kxkb_write("LayoutList", layout)?;
    // One layout only: no stale display name left over from a previous list
    kxkb_write("DisplayNames", "")?;
    if let Some(v) = variant {
        kxkb_write("VariantList", v)?;
    }
    kxkb_reload()?;

    // Give KWin a moment to re-read kxkbrc, then select the first (only) layout explicitly
    // and report what KWin says: this tells whether KWin really took the change into account.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let _ = kwin_keyboard_call("org.kde.KeyboardLayouts.setLayout", &["uint32:0"]);
    match kwin_keyboard_call("org.kde.KeyboardLayouts.getLayoutsList", &[]) {
        Some(list) => eprintln!("[keyboard] KWin : dispositions actives = {list:?} (demandé : {layout} {variant:?})"),
        None => eprintln!("[keyboard] KWin : service org.kde.keyboard sans réponse (D-Bus)"),
    }
    Ok(())
}

/// The layout in use now (first of the list) and its variant: the Plasma one (kxkbrc),
/// else the system one (localectl). `None` when none is set
pub fn current_layout() -> Option<(String, String)> {
    let first = |list: &str| list.split(',').next().unwrap_or("").trim().to_string();
    let layout = first(&kxkb_read("LayoutList"));
    if !layout.is_empty() {
        return Some((layout, first(&kxkb_read("VariantList"))));
    }
    let out = Command::new("localectl").arg("status").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .map(|v| first(v))
            .unwrap_or_default()
    };
    let layout = field("X11 Layout:");
    (!layout.is_empty() && layout != "(unset)").then(|| (layout, field("X11 Variant:")))
}

fn kxkb_save_backup_once() {
    if let Ok(mut guard) = LIVE_BACKUP.lock() {
        if guard.is_none() {
            *guard = Some(KxkbSnapshot {
                use_flag: kxkb_read("Use"),
                layout_list: kxkb_read("LayoutList"),
                variant_list: kxkb_read("VariantList"),
            });
        }
    }
}

/// Wayland: puts back the session layout saved before the live tests, unless the
/// configuration has been persisted in the meantime. To be called when the window closes.
pub fn restore_live_layout() {
    if !is_wayland_session() || PERSISTED.load(Ordering::SeqCst) {
        return;
    }
    let snapshot = LIVE_BACKUP.lock().ok().and_then(|mut g| g.take());
    if let Some(b) = snapshot {
        let use_flag = if b.use_flag.is_empty() { "false" } else { b.use_flag.as_str() };
        let _ = kxkb_write("Use", use_flag)
            .and_then(|_| kxkb_write("LayoutList", &b.layout_list))
            .and_then(|_| kxkb_write("VariantList", &b.variant_list))
            .and_then(|_| kxkb_reload());
    }
}

// ---------------------------------------------------------------------------
// Live application (test) and persistence
// ---------------------------------------------------------------------------

/// Returns `variant` only if it really exists for `layout` (an invalid pair such as
/// "us" + "fr" makes xkb refuse the keymap and the layout silently stays unchanged).
fn valid_variant<'a>(layout: &str, variant: Option<&'a str>) -> Option<&'a str> {
    let v = variant.map(str::trim).filter(|v| !v.is_empty())?;
    match list_variants(layout) {
        Ok(list) if list.iter().any(|x| x.code == v) => Some(v),
        Ok(_) => {
            eprintln!("[keyboard] variante « {v} » inexistante pour la disposition « {layout} » : ignorée");
            None
        }
        // base.lst illisible : on ne bloque rien
        Err(_) => Some(v),
    }
}

/// Runs `setxkbmap` (X11 / Xwayland keymap). `quiet` hides its stderr: under Wayland it
/// prints "Running setxkbmap against an Xwayland server", which is expected there since
/// the Wayland session itself is handled through kxkbrc.
fn run_setxkbmap(layout: &str, variant: Option<&str>, quiet: bool) -> std::io::Result<()> {
    let mut cmd = Command::new("setxkbmap");
    cmd.arg(layout);
    if let Some(v) = variant {
        cmd.arg("-variant").arg(v);
    }
    if quiet {
        cmd.stderr(Stdio::null());
    }
    let status = cmd.status()?;
    if !status.success() {
        return Err(other("setxkbmap a échoué"));
    }
    Ok(())
}

/// Applies a keyboard layout right away, so that the user can test it.
/// - Wayland (Plasma): kxkbrc + `org.kde.keyboard.reloadConfig` (the original values are
///   saved and put back by `restore_live_layout` if the user quits without applying).
///   If the test area is an X11 client running under Xwayland (QT_QPA_PLATFORM=xcb), the
///   Xwayland keymap is set as well with `setxkbmap` (silenced).
/// - X11: `setxkbmap`.
/// Not a persistent system configuration: `persist_layout` handles that.
/// Every failure is printed on stderr (run the application from a terminal to see it).
pub fn apply_layout_live(layout: &str, variant: Option<&str>) -> std::io::Result<()> {
    let variant = valid_variant(layout, variant);
    if !is_wayland_session() {
        return run_setxkbmap(layout, variant, false).map_err(|e| {
            eprintln!("[keyboard] setxkbmap ({layout}): {e}");
            e
        });
    }

    kxkb_save_backup_once();
    let kde = kxkb_apply(layout, variant);
    match &kde {
        Ok(()) => eprintln!("[keyboard] Wayland: kxkbrc + reloadConfig OK ({layout} {variant:?})"),
        Err(e) => eprintln!("[keyboard] Wayland: échec KDE ({layout} {variant:?}): {e}"),
    }

    // Xwayland clients read the Xwayland keymap
    let xwayland = if std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty()) {
        let r = run_setxkbmap(layout, variant, true);
        if let Err(e) = &r {
            eprintln!("[keyboard] Wayland: setxkbmap (Xwayland) ({layout}): {e}");
        }
        r
    } else {
        Err(other("DISPLAY absent"))
    };

    // OK if at least one of the two routes worked
    kde.or(xwayland)
}

/// Writes the final configuration using localectl (persists in
/// /etc/vconsole.conf and /etc/X11/xorg.conf.d/00-keyboard.conf).
/// Requires the appropriate permissions (polkit / root).
/// On Wayland, the Plasma session layout (kxkbrc) is also written: this is what KWin reads.
pub fn persist_layout(layout: &str, variant: Option<&str>) -> std::io::Result<()> {
    let variant = valid_variant(layout, variant);
    if is_wayland_session() {
        kxkb_apply(layout, variant)?;
        PERSISTED.store(true, Ordering::SeqCst);
    }

    let mut cmd = Command::new("localectl");
    cmd.arg("set-x11-keymap").arg(layout);
    if let Some(v) = variant {
        cmd.arg("").arg(v);
    }
    let status = cmd.status()?;
    if !status.success() {
        return Err(other("localectl set-x11-keymap a échoué (droits insuffisants ?)"));
    }
    Ok(())
}

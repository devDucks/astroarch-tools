//! Explore available keyboard layouts via xkeyboard-config
//! and apply them in real time to the test area.
//!
//! Ref: https://wiki.archlinux.org/title/Linux_console/Keyboard_configuration
//! (console) — Under Plasma/X11/Wayland, configuration is done via
//! `localectl set-x11-keymap` (+ kxkbrc under Wayland); the console configuration (vconsole.conf) is
//! generated separately using `localectl set-keymap` if necessary

use std::process::{Command, Stdio};
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

/// base.lst, read once: the proposals and their names look it up at every change
fn base_lst() -> std::io::Result<&'static str> {
    static CONTENT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(content) = CONTENT.get() {
        return Ok(content);
    }
    let content = std::fs::read_to_string(XKB_BASE_LST)?;
    Ok(CONTENT.get_or_init(|| content))
}

/// Parses the “! layout” section of base.lst
pub fn list_layouts() -> std::io::Result<Vec<KeyboardLayout>> {
    parse_section(base_lst()?, "! layout").map(|rows| {
        rows.into_iter()
            .map(|(code, description)| KeyboardLayout { code, description })
            .collect()
    })
}

/// Parses the “! variant” section of base.lst, filtered by a given layout.
/// The line format is: “variant  layout: description”
pub fn list_variants(layout: &str) -> std::io::Result<Vec<KeyboardVariant>> {
    let content = base_lst()?;
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

fn parse_section(content: &str, section_header: &str) -> std::io::Result<Vec<(String, String)>> {
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

/// Countries whose XKB layout is not their lowercase code (or has none of that name)
const COUNTRY_LAYOUTS: &[(&str, &str)] = &[
    ("GB", "gb"), ("AU", "us"), ("NZ", "us"), ("HK", "us"), ("SG", "us"), ("ID", "us"),
    ("SA", "ara"), ("EG", "ara"), ("AE", "ara"), ("QA", "ara"), ("KW", "ara"), ("BH", "ara"),
    ("OM", "ara"), ("JO", "ara"), ("LB", "ara"), ("LY", "ara"), ("SD", "ara"), ("YE", "ara"),
    ("MX", "latam"), ("AR", "latam"), ("CL", "latam"), ("CO", "latam"), ("PE", "latam"),
    ("VE", "latam"), ("UY", "latam"), ("EC", "latam"), ("BO", "latam"), ("PY", "latam"),
    ("CR", "latam"), ("GT", "latam"), ("PA", "latam"), ("DO", "latam"), ("HN", "latam"),
    ("SV", "latam"), ("NI", "latam"), ("CU", "latam"),
    ("CZ", "cz"), ("DK", "dk"), ("EE", "ee"), ("GR", "gr"), ("IR", "ir"), ("JP", "jp"),
    ("KR", "kr"), ("SE", "se"), ("SI", "si"), ("UA", "ua"), ("VN", "vn"),
];

/// Languages without a layout of their own country to follow, to the layout of their
/// country of origin (the language of a locale without a layout, "fr_LU", "en_CH")
const LANGUAGE_LAYOUTS: &[(&str, &str)] = &[
    ("en", "us"), ("fr", "fr"), ("de", "de"), ("es", "es"), ("it", "it"), ("pt", "pt"),
    ("nl", "nl"), ("ru", "ru"), ("ja", "jp"), ("ko", "kr"), ("zh", "cn"), ("ar", "ara"),
    ("he", "il"), ("el", "gr"), ("uk", "ua"), ("pl", "pl"), ("cs", "cz"), ("sv", "se"),
    ("da", "dk"), ("nb", "no"), ("nn", "no"), ("fi", "fi"), ("tr", "tr"), ("hu", "hu"),
    ("ro", "ro"), ("bg", "bg"), ("sk", "sk"), ("sl", "si"), ("hr", "hr"), ("sr", "rs"),
    ("et", "ee"), ("lv", "lv"), ("lt", "lt"), ("is", "is"), ("ga", "ie"), ("hi", "in"),
    ("th", "th"), ("vi", "vn"), ("fa", "ir"), ("gsw", "ch"),
];

/// A layout and its variant (empty for the default one)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardChoice {
    pub layout: String,
    pub variant: String,
}

/// The XKB layout of a country: its lowercase code ("it", "ch") unless COUNTRY_LAYOUTS says
/// otherwise. `layouts`: the layouts base.lst has (empty: not checked)
fn country_layout(country: &str, layouts: &[String]) -> Option<String> {
    let layout = COUNTRY_LAYOUTS
        .iter()
        .find(|(c, _)| *c == country)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| country.to_lowercase());
    (layouts.is_empty() || layouts.contains(&layout)).then_some(layout)
}

/// The variant of a layout that is made for a language: the one named like it ("fr" of
/// "ch"), or one set here ("eng" of "ca"). `variants`: the variants of the layout
fn language_variant(layout: &str, language: &str, variants: &[String]) -> Option<String> {
    let wanted = match (layout, language) {
        ("ca", "en") => "eng",
        _ => language,
    };
    variants.iter().find(|v| *v == wanted).cloned()
}

/// Keyboards to propose for a country and a language (a locale, "fr_CH"), best first:
/// - what the language is typed on in its own country ("en_GB": gb), then the country's
///   when the language is a foreign one in this country, the country's first when it
///   is one of its own (the user's keyboard may come from another country);
/// - the country's default layout, when its variant for the language is proposed.
/// `layouts`: the layouts of base.lst; `variants_of`: the variants of a layout
pub fn suggest(
    country: &str,
    locale: &str,
    layouts: &[String],
    variants_of: &dyn Fn(&str) -> Vec<String>,
) -> Vec<KeyboardChoice> {
    let base = locale.split('@').next().unwrap_or(locale);
    let base = base.split('.').next().unwrap_or(base);
    let (language, home) = base.split_once('_').unwrap_or((base, ""));
    // No country chosen yet (the keyboard page alone): the language's own
    let country = if country.is_empty() { home } else { country };

    let build = |from: &str| -> Option<KeyboardChoice> {
        let layout = country_layout(from, layouts)?;
        let variant = language_variant(&layout, language, &variants_of(&layout)).unwrap_or_default();
        Some(KeyboardChoice { layout, variant })
    };
    let own = build(country);
    let origin = build(home).or_else(|| {
        let layout = LANGUAGE_LAYOUTS.iter().find(|(l, _)| *l == language).map(|(_, l)| l.to_string())?;
        (layouts.is_empty() || layouts.contains(&layout)).then(|| KeyboardChoice { layout, variant: String::new() })
    });

    let native = home == country;
    let mut out: Vec<KeyboardChoice> = Vec::new();
    let mut push = |c: Option<KeyboardChoice>| {
        if let Some(c) = c {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    };
    if native {
        push(own.clone());
        push(origin);
    } else {
        push(origin);
        push(own.clone());
    }
    // The default layout of the country, when its variant for the language comes first
    if let Some(c) = own {
        push(Some(KeyboardChoice { layout: c.layout, variant: String::new() }));
    }
    out
}

/// Readable name of a layout and its variant, from base.lst ("French (Switzerland) ·
/// French"); the codes when base.lst does not name them
pub fn describe(layout: &str, variant: &str) -> String {
    let name = list_layouts()
        .ok()
        .and_then(|l| l.into_iter().find(|l| l.code == layout))
        .map(|l| l.description)
        .unwrap_or_else(|| layout.to_string());
    if variant.is_empty() {
        return name;
    }
    let variant_name = list_variants(layout)
        .ok()
        .and_then(|v| v.into_iter().find(|v| v.code == variant))
        .map(|v| v.description)
        .unwrap_or_else(|| variant.to_string());
    format!("{name} · {variant_name}")
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

/// The session layout, saved before the first live change so that it can be put back if
/// the user leaves without applying: the whole `[Layout]` group of ~/.config/kxkbrc that a
/// live change writes (Wayland), or the X server's keymap (X11, `setxkbmap -query`).
/// Several layouts and their switch option are kept as they were.
#[derive(Debug, Clone)]
enum LiveSnapshot {
    Kxkb { use_flag: String, layout_list: String, variant_list: String, display_names: String },
    X11 { model: String, layout: String, variant: String, options: String },
}

/// Taken by the first live change, dropped when the layout is put back or applied
static LIVE_BACKUP: Mutex<Option<LiveSnapshot>> = Mutex::new(None);

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

/// The X server's keymap now (`setxkbmap -query`)
fn x11_query() -> Option<LiveSnapshot> {
    let out = Command::new("setxkbmap").arg("-query").output().ok().filter(|o| o.status.success())?;
    parse_x11_query(&String::from_utf8_lossy(&out.stdout))
}

/// "rules: evdev / model: pc105 / layout: us,it / variant: , / options: grp:alt_shift_toggle"
fn parse_x11_query(text: &str) -> Option<LiveSnapshot> {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    let layout = field("layout:");
    (!layout.is_empty()).then(|| LiveSnapshot::X11 {
        model: field("model:"),
        layout,
        variant: field("variant:"),
        options: field("options:"),
    })
}

fn save_live_backup_once() {
    if let Ok(mut guard) = LIVE_BACKUP.lock() {
        if guard.is_none() {
            *guard = if is_wayland_session() {
                Some(LiveSnapshot::Kxkb {
                    use_flag: kxkb_read("Use"),
                    layout_list: kxkb_read("LayoutList"),
                    variant_list: kxkb_read("VariantList"),
                    display_names: kxkb_read("DisplayNames"),
                })
            } else {
                x11_query()
            };
        }
    }
}

/// The layout now applied is the one to keep: nothing to put back any more
fn forget_live_backup() {
    if let Ok(mut guard) = LIVE_BACKUP.lock() {
        *guard = None;
    }
}

/// Puts back the session layout saved before the live tests, when a layout was tried and
/// not applied since (otherwise nothing to do). To be called when the user leaves without
/// applying: Cancel, the window closing.
pub fn restore_live_layout() {
    let snapshot = LIVE_BACKUP.lock().ok().and_then(|mut g| g.take());
    match snapshot {
        Some(LiveSnapshot::Kxkb { use_flag, layout_list, variant_list, display_names }) => {
            let use_flag = if use_flag.is_empty() { "false" } else { use_flag.as_str() };
            // Same order as kxkb_apply: never a layout with the variant of another one
            let result = kxkb_write("Use", use_flag)
                .and_then(|_| kxkb_write("VariantList", ""))
                .and_then(|_| kxkb_write("LayoutList", &layout_list))
                .and_then(|_| kxkb_write("DisplayNames", &display_names))
                .and_then(|_| kxkb_write("VariantList", &variant_list))
                .and_then(|_| kxkb_reload());
            if let Err(e) = result {
                eprintln!("[keyboard] Wayland: the session layout could not be put back: {e}");
            }
        }
        Some(LiveSnapshot::X11 { model, layout, variant, options }) => {
            let mut cmd = Command::new("setxkbmap");
            if !model.is_empty() {
                cmd.args(["-model", model.as_str()]);
            }
            // "-option ''" clears the options before they are set again
            cmd.args(["-layout", layout.as_str(), "-variant", variant.as_str(), "-option", ""]);
            for option in options.split(',').filter(|o| !o.is_empty()) {
                cmd.args(["-option", option]);
            }
            if !cmd.status().is_ok_and(|s| s.success()) {
                eprintln!("[keyboard] setxkbmap: the session layout could not be put back ({layout})");
            }
        }
        None => {}
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

/// Applies a keyboard layout right away, so that the user can test it. The session layout
/// is saved first (once) and put back by `restore_live_layout` if the user leaves without
/// applying. Not a persistent system configuration: `persist_layout` handles that.
pub fn apply_layout_live(layout: &str, variant: Option<&str>) -> std::io::Result<()> {
    let variant = valid_variant(layout, variant);
    save_live_backup_once();
    set_session_layout(layout, variant)
}

/// The layout of the session:
/// - Wayland (Plasma): kxkbrc + `org.kde.keyboard.reloadConfig`. If the test area is an X11
///   client running under Xwayland (QT_QPA_PLATFORM=xcb), the Xwayland keymap is set as
///   well with `setxkbmap` (silenced).
/// - X11: `setxkbmap`.
/// Every failure is printed on stderr (run the application from a terminal to see it).
fn set_session_layout(layout: &str, variant: Option<&str>) -> std::io::Result<()> {
    if !is_wayland_session() {
        return run_setxkbmap(layout, variant, false).map_err(|e| {
            eprintln!("[keyboard] setxkbmap ({layout}): {e}");
            e
        });
    }

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

/// Writes the final configuration: first the system one with localectl (persists in
/// /etc/vconsole.conf and /etc/X11/xorg.conf.d/00-keyboard.conf; polkit may ask for a
/// password), then, only once it is written, the layout of the session (on Wayland the
/// Plasma one in kxkbrc, which KWin reads at the next login too). A refused or failed
/// localectl changes nothing.
pub fn persist_layout(layout: &str, variant: Option<&str>) -> std::io::Result<()> {
    let variant = valid_variant(layout, variant);
    let mut cmd = Command::new("localectl");
    cmd.arg("set-x11-keymap").arg(layout);
    if let Some(v) = variant {
        cmd.arg("").arg(v);
    }
    let status = cmd.status()?;
    if !status.success() {
        return Err(other("localectl set-x11-keymap a échoué (droits insuffisants ?)"));
    }

    let session = set_session_layout(layout, variant);
    // The session uses the applied layout now: a layout tried before is not put back
    forget_live_backup();
    match session {
        // kxkbrc is the layout of the next Plasma sessions
        Err(e) if is_wayland_session() => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_keymap_is_kept_whole() {
        let query = "rules:      evdev\nmodel:      pc105\nlayout:     us,it\nvariant:    ,\noptions:    grp:alt_shift_toggle,compose:ralt\n";
        match parse_x11_query(query) {
            Some(LiveSnapshot::X11 { model, layout, variant, options }) => {
                assert_eq!(model, "pc105");
                assert_eq!(layout, "us,it");
                assert_eq!(variant, ",");
                assert_eq!(options, "grp:alt_shift_toggle,compose:ralt");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(parse_x11_query("rules: evdev\nmodel: pc105\n").is_none());
    }

    fn layouts(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    fn pair(layout: &str, variant: &str) -> KeyboardChoice {
        KeyboardChoice { layout: layout.into(), variant: variant.into() }
    }

    fn variants(layout: &str) -> Vec<String> {
        match layout {
            "ch" => layouts(&["de_nodeadkeys", "fr", "fr_mac"]),
            "ca" => layouts(&["fr-legacy", "eng", "multix"]),
            _ => Vec::new(),
        }
    }

    #[test]
    fn swiss_french_gets_the_french_variant_of_the_swiss_layout() {
        let all = layouts(&["ch", "fr", "de", "us", "gb"]);
        assert_eq!(suggest("CH", "fr_CH", &all, &variants), vec![pair("ch", "fr"), pair("ch", "")]);
        assert_eq!(suggest("CH", "de_CH", &all, &variants), vec![pair("ch", "")]);
    }

    #[test]
    fn a_foreign_language_proposes_its_own_keyboard_first() {
        let all = layouts(&["ch", "fr", "de", "us", "gb"]);
        assert_eq!(suggest("CH", "en_GB", &all, &variants), vec![pair("gb", ""), pair("ch", "")]);
        assert_eq!(suggest("FR", "en_US", &all, &variants), vec![pair("us", ""), pair("fr", "")]);
    }

    #[test]
    fn canada_has_a_variant_per_language() {
        let all = layouts(&["ca", "us"]);
        assert_eq!(suggest("CA", "en_CA", &all, &variants), vec![pair("ca", "eng"), pair("ca", "")]);
        assert_eq!(suggest("CA", "fr_CA", &all, &variants), vec![pair("ca", "")]);
    }

    #[test]
    fn a_locale_without_a_layout_follows_its_language() {
        let all = layouts(&["fr", "de", "us"]);
        // fr_LU: no "lu" layout, the French one
        assert_eq!(suggest("LU", "fr_LU", &all, &variants), vec![pair("fr", "")]);
    }

    #[test]
    fn without_a_country_the_language_decides() {
        let all = layouts(&["ch", "us"]);
        assert_eq!(suggest("", "fr_CH", &all, &variants), vec![pair("ch", "fr"), pair("ch", "")]);
    }

    #[test]
    fn the_layouts_of_country_codes_exist_or_are_dropped() {
        let all = layouts(&["gb", "latam", "us"]);
        assert_eq!(country_layout("GB", &all).unwrap(), "gb");
        assert_eq!(country_layout("MX", &all).unwrap(), "latam");
        assert!(country_layout("LU", &all).is_none());
    }
}



use cxx_qt_build::{CxxQtBuilder, QmlModule};
use std::path::PathBuf;
use std::process::Command;

/// Searches for the Qt `lrelease` tool: first via the LRELEASE variable, then by checking the Qt binaries directory
/// as indicated by qmake (using the QMAKE variable, or by trying `qmake6`, `qmake-qt6`, or `qmake`), and finally by checking standard names.
pub fn find_lrelease() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LRELEASE") {
        return Some(PathBuf::from(p));
    }

    let mut qmakes: Vec<String> = Vec::new();
    if let Ok(q) = std::env::var("QMAKE") {
        qmakes.push(q);
    }
    qmakes.extend(["qmake6", "qmake-qt6", "qmake"].map(String::from));

    let mut bin_dirs: Vec<PathBuf> = Vec::new();
    for q in qmakes {
        if let Ok(out) = Command::new(&q).args(["-query", "QT_INSTALL_BINS"]).output() {
            if out.status.success() {
                let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !dir.is_empty() {
                    bin_dirs.push(PathBuf::from(dir));
                }
            }
        }
    }
    bin_dirs.push(PathBuf::from("/usr/lib/qt6/bin"));

    let names = ["lrelease", "lrelease-qt6", "lrelease6"];
    for dir in &bin_dirs {
        for name in names {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    // Dernier recours : le PATH
    names.iter().map(PathBuf::from).find(|c| {
        Command::new(c)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Compiles each `<ts_dir>/*.ts` file into `<stem>.qm` (within OUT_DIR) and generates a `.qrc` file
/// that includes them under `qrc_prefix` (`<file alias="x.qm">x.qm</file>`)
/// Returns `None` if `lrelease` cannot be found or if no files could be compiled;
/// in such cases, the build process continues, and the interface will remain in English
pub fn translations_qrc(ts_dir: &str, qrc_prefix: &str) -> Option<PathBuf> {
    println!("cargo:rerun-if-changed={ts_dir}");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));

    let lrelease = match find_lrelease() {
        Some(l) => l,
        None => {
            println!(
                "cargo:warning=lrelease introuvable (paquet qt6-tools) : traductions non incluses (interface en anglais)"
            );
            return None;
        }
    };

    let mut ts_files: Vec<PathBuf> = std::fs::read_dir(ts_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ts"))
        .collect();
    ts_files.sort();

    let mut files = String::new();
    for path in ts_files {
        let stem = path.file_stem()?.to_string_lossy().to_string();
        let qm_name = format!("{stem}.qm");
        let status = Command::new(&lrelease)
            .arg(&path)
            .arg("-qm")
            .arg(out_dir.join(&qm_name))
            .arg("-silent")
            .status();
        match status {
            Ok(s) if s.success() => {
                files.push_str(&format!("    <file alias=\"{0}\">{0}</file>\n", qm_name));
            }
            _ => println!("cargo:warning=échec de lrelease sur {}", path.display()),
        }
    }
    if files.is_empty() {
        return None;
    }

    // Unique .qrc name per prefix (multiple calls possible within a single build.rs)
    let qrc = out_dir.join("translations.qrc");
    let content = format!(
        "<RCC>\n  <qresource prefix=\"{qrc_prefix}\">\n{files}  </qresource>\n</RCC>\n"
    );
    std::fs::write(&qrc, content).ok()?;
    Some(qrc)
}

/// Translations `translations/qml_<language>.ts` placed where `QQmlApplicationEngine`
/// automatically looks for them: `qrc:/qt/qml/<URI>/qml/i18n/qml_<language>.qm`
/// (alongside Main.qml; loaded at startup and subsequently whenever `Qt.uiLanguage` changes).
pub fn qml_i18n_qrc(module_uri: &str) -> Option<PathBuf> {
    // The dots in the URI become "/" in the qrc path (com.astroarch.x -> com/astroarch/x)
    let path = module_uri.replace('.', "/");
    translations_qrc("translations", &format!("/qt/qml/{path}/qml/i18n"))
}

/// Declares a QML module + Rust objects (`#[cxx_qt::bridge]`) and triggers CXX-Qt
/// code generation. The .qml and .rs files are specified relative to the crate root.
pub fn build_qml_app(
    module_uri: &str,
    qml_files: &[&str],
    rust_files: &[&str],
    qrc: Option<PathBuf>,
) {
    println!("cargo:rerun-if-changed=build.rs");

    let mut builder = CxxQtBuilder::new_qml_module(
        QmlModule::new(module_uri).qml_files(qml_files.iter().copied()),
    )
    .qt_module("Qml")
    .qt_module("Quick")
    // Our .qml files use QtQuick.Controls = QuickControls2
    .qt_module("QuickControls2")
    .files(rust_files.iter().copied());

    if let Some(qrc) = qrc {
        builder = builder.qrc(qrc);
    }

    builder.build();
}

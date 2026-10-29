//! Aides partagées pour les `build.rs` des applis AstroArch (CXX-Qt 0.10).
//!
//! Avant : chaque appli embarquait sa propre copie de `find_lrelease()` et de la
//! compilation des traductions. Maintenant, un `build.rs` tient en quelques lignes :
//!
//! ```no_run
//! fn main() {
//!     let uri = "AstroArchBackup";
//!     astroarch_build::build_qml_app(
//!         uri,
//!         &["qml/Main.qml"],
//!         &["src/bridge.rs"],
//!         astroarch_build::qml_i18n_qrc(uri),
//!     );
//! }
//! ```

use cxx_qt_build::{CxxQtBuilder, QmlModule};
use std::path::PathBuf;
use std::process::Command;

/// Cherche l'outil Qt `lrelease` : variable LRELEASE, puis le dossier des binaires Qt
/// indiqué par qmake (variable QMAKE, qmake6, qmake-qt6, qmake), puis les noms usuels.
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

/// Compile chaque `<ts_dir>/*.ts` en `<stem>.qm` (dans OUT_DIR) et écrit un fichier .qrc
/// qui les place sous `qrc_prefix` (`<file alias="x.qm">x.qm</file>`).
/// Renvoie `None` si `lrelease` est introuvable ou si rien n'a pu être compilé :
/// la compilation continue et l'interface reste alors en anglais.
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

    // Nom de .qrc unique par préfixe (plusieurs appels possibles dans un même build.rs)
    let qrc = out_dir.join("translations.qrc");
    let content = format!(
        "<RCC>\n  <qresource prefix=\"{qrc_prefix}\">\n{files}  </qresource>\n</RCC>\n"
    );
    std::fs::write(&qrc, content).ok()?;
    Some(qrc)
}

/// Traductions `translations/qml_<langue>.ts` placées là où `QQmlApplicationEngine`
/// les cherche automatiquement : `qrc:/qt/qml/<URI>/qml/i18n/qml_<langue>.qm`
/// (à côté de Main.qml, au démarrage puis à chaque changement de `Qt.uiLanguage`).
pub fn qml_i18n_qrc(module_uri: &str) -> Option<PathBuf> {
    // Les points de l'URI deviennent des « / » dans le chemin qrc (com.astroarch.x -> com/astroarch/x)
    let path = module_uri.replace('.', "/");
    translations_qrc("translations", &format!("/qt/qml/{path}/qml/i18n"))
}

/// Déclare un module QML + les objets Rust (`#[cxx_qt::bridge]`) et lance la génération
/// CXX-Qt. Les fichiers .qml et .rs sont donnés relativement à la racine du crate.
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
    // Nos fichiers .qml utilisent QtQuick.Controls = QuickControls2
    .qt_module("QuickControls2")
    .files(rust_files.iter().copied());

    if let Some(qrc) = qrc {
        builder = builder.qrc(qrc);
    }

    builder.build();
}

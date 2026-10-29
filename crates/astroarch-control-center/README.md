# astroarch-control-center

AstroArch Control Center: a window containing buttons that launch system tools.

**100% Rust** (with a QML interface): no remaining C++ or CMake files. Compiled using only Cargo.

## Compiling and installing

cargo build --release
sudo ./install.sh              # Defaults to PREFIX=/usr

Prerequisites: Qt 6 (Quick Controls 2), `qt6-tools` (for `lrelease`, which compiles
translations—without it, the interface will remain in English), Konsole (or another terminal),
and zsh (for `update-astroarch`).

## Buttons

| Button | Command | Mode |
|--------|----------|------|
| System Update | `update-astroarch` | Separate console (interactive zsh; uses a zsh plugin) |
| Language & Keyboard | `astroarch-locale-module` | Application |
| Services | `astroarch-sys-manager` | Application |
| Backup | `astroarch-backup` | Application |
| Astro Monitor | `astromonitor` | Separate console |
| Software | `astroarch-software-installer` | Application |

To add or modify a button, edit the `entries` list in `qml/Main.qml`; no changes to the Rust code are required.

## Translations

Uses `qsTr()` within the QML files. Translation files (`translations/qml_<language>.ts` — fr, de, es, it) are compiled by
`build.rs` using `lrelease` and are automatically loaded based on the system language.
After modifying any text in the QML files:

    lupdate-qt6 qml -ts translations/qml_fr.ts translations/qml_de.ts translations/qml_es.ts translations/qml_it.ts

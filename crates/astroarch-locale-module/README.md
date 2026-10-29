# astroarch-locale-module

AstroArch language/location/keyboard configuration module.
**100% Rust** application (Rust + cxx-qt 0.10 + QML), compiled with `cargo` alone:
no more CMake, `main.cpp` or `translator_manager`.

## Compile and launch 

cargo build --release 
./target/release/astroarch-locale-module

Prerequisites: Qt 6 (Qml, Quick, QuickControls2, Shapes), and the `lrelease` tool
(Qt Linguist package) to compile the translations. Without `lrelease`, the
compilation continues with a warning and the interface remains in English.
`build.rs` searches for `lrelease` via the `LRELEASE` variable, then `qmake6 -query`.

## Structure

Cargo.toml, build.rs      build.rs declares the QML module and compiles translations/*.ts
src/main.rs               QGuiApplication + QQmlApplicationEngine, nothing else
src/bridge.rs             LocaleBridge QObject exposed to QML (via cxx-qt)
src/locale_gen.rs         locale.gen, language/country names, persist_locale (using pkexec)
src/keyboard.rs           XKB layouts, live testing (setxkbmap on X11; kxkbrc + D-Bus on Wayland), localectl (for persistence)
src/geolocation.rs        Country/timezone detection via IP (ip-api.com)
qml/                      Main, LanguagePage, LocationPage, KeyboardPage, WorldMapView
translations/qml_<langue>.ts   fr, it, es, de + en
gen_map.py, map-source/   Regenerates country outlines for WorldMapView.qml
(uses Natural Earth data, public domain)

## Translations

Text strings are written using `qsTr()` in QML. The `build.rs` script compiles each `translations/qml_<language>.ts` file into a `.qm` file and places it in `qrc:/qt/qml/AstroArchLocaleModule/qml/i18n/`, from where `QQmlApplicationEngine` loads them automatically (based on the system language at startup).

Changing the language at runtime is handled in QML via `Qt.uiLanguage` (see `Main.qml`): Qt loads the corresponding `qml_<language>.qm` file and re-evaluates all `qsTr()` calls. The `qml_en.ts` file (containing the source strings) allows for switching back to English; for any language lacking a corresponding `.ts` file, the system defaults to English (`en`).

To add a language: create a `translations/qml_xx.ts` file (by copying an existing `.ts` file and translating its contents), then add `"xx"` to the `translatedLanguages` list in `Main.qml`.

## Pre-filling and application of choices

Upon launch, the system language is set, followed by country, keyboard layout, and language—determined based on the IP address, provided a network connection is available. Selecting "Finish" triggers an authorization request (via `pkexec`), then enables the locale in `/etc/locale.gen`, runs `locale-gen`, applies the settings using `localectl set-locale` and `localectl set-x11-keymap`, and updates `~/.config/plasma-localerc`. You must log out and log back in for the new language to take effect in applications that are already open (a full system reboot is not required).

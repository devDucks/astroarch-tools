# astroarch-locale-module

AstroArch language/location/keyboard configuration module.
**100% Rust** application (Rust + cxx-qt 0.10 + QML), compiled with `cargo` alone:
no more CMake, `main.cpp` or `translator_manager`.

**Menu entry**: `org.astroarch.locale-module.desktop` now opens the Language and time zone
page of the AstroArch Control Center (`astroarch-control-center --page language --single`):
its country, language and keyboard steps use this crate's code (`bridge.rs`, `keyboard.rs`,
`locale_gen.rs`, `geolocation.rs`, compiled in). The standalone binary still builds.

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
src/geolocation.rs        Country detection via IP (geoip.kde.org); cargo feature `geoip`
                          (default; reqwest), off in the AstroArch Control Center
src/countries.rs          Languages of a country: request to https://restcountries.com (curl), matched
                          with the locales of locale.gen; without network, the locales of locale.gen
qml/                      Main, LanguagePage, LocationPage, KeyboardPage, WorldMapView
translations/qml_<langue>.ts   fr, it, es, de + en
gen_map.py, map-source/   Regenerates country outlines for WorldMapView.qml
(uses Natural Earth data, public domain)

## Translations

Text strings are written using `qsTr()` in QML. The `build.rs` script compiles each `translations/qml_<language>.ts` file into a `.qm` file and places it in `qrc:/qt/qml/AstroArchLocaleModule/qml/i18n/`, from where `QQmlApplicationEngine` loads them automatically (based on the system language at startup).

Changing the language at runtime is handled in QML via `Qt.uiLanguage` (see `Main.qml`): Qt loads the corresponding `qml_<language>.qm` file and re-evaluates all `qsTr()` calls. The `qml_en.ts` file (containing the source strings) allows for switching back to English; for any language lacking a corresponding `.ts` file, the system defaults to English (`en`).

To add a language: create a `translations/qml_xx.ts` file (by copying an existing `.ts` file and translating its contents), then add `"xx"` to the `translatedLanguages` list in `Main.qml`.

## Proposals and application of choices

Three steps, each one a **proposal** the user confirms or changes. Nothing is decided
automatically from a table.

1. **Country**: proposed from the IP address (`geolocation.rs`) when a network is available;
   the user keeps it or picks another one (list or map).
2. **Language**: the languages of the country are asked of `https://restcountries.com`
   (`countries.rs`, only the country code is sent) and matched with the locales `locale.gen`
   can generate. A single language is selected; with several, the user chooses; "Another
   language…" gives the whole list (a foreigner, a language that is not the country's). Without
   a network, the proposals are the locales `locale.gen` has for the country.
3. **Keyboard**: layouts proposed from the country and the language (`keyboard.rs`, `suggest`):
   the country's layout with the variant made for the language (Swiss French: `ch` + `fr`), the
   layout the language is typed on in its own country (English: `us` or `gb`), the country's
   default layout. "Another keyboard…" gives every layout and variant (a keyboard from another
   country). A keyboard the user chose is never replaced by a later proposal.

Selecting "Finish" triggers an authorization request (via `pkexec`), then enables the locale in `/etc/locale.gen`, runs `locale-gen`, applies the settings using `localectl set-locale` and `localectl set-x11-keymap`, and updates `~/.config/plasma-localerc`. You must log out and log back in for the new language to take effect in applications that are already open (a full system reboot is not required).

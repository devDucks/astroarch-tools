# astroarch-locale-module

Module de configuration langue / emplacement / clavier d'AstroArch.
Application **100 % Rust** (Rust + cxx-qt 0.10 + QML), compilée avec `cargo` seul :
plus de CMake, de `main.cpp` ni de `translator_manager`.

## Compiler et lancer

    cargo build --release
    ./target/release/astroarch-locale-module

Prérequis : Qt 6 (Qml, Quick, QuickControls2, Shapes), et l'outil `lrelease`
(paquet Qt Linguist) pour compiler les traductions. Sans `lrelease`, la
compilation continue avec un avertissement et l'interface reste en anglais.
`build.rs` cherche `lrelease` via la variable `LRELEASE`, puis `qmake6 -query`.

## Structure

    Cargo.toml, build.rs      build.rs déclare le module QML et compile translations/*.ts
    src/main.rs               QGuiApplication + QQmlApplicationEngine, rien d'autre
    src/bridge.rs             QObject LocaleBridge exposé à QML (cxx-qt)
    src/locale_gen.rs         locale.gen, noms de langues/pays, persist_locale (pkexec)
    src/keyboard.rs           layouts xkb, test en direct (setxkbmap sous X11, kxkbrc + D-Bus sous Wayland), localectl (persistance)
    src/geolocation.rs        pays/fuseau via l'IP (ip-api.com)
    qml/                      Main, LanguagePage, LocationPage, KeyboardPage, WorldMapView
    translations/qml_<langue>.ts   fr, it, es, de + en
    gen_map.py, map-source/   régénère les contours de pays de WorldMapView.qml
                              (données Natural Earth, domaine public)

## Traductions

Les textes sont écrits avec `qsTr()` dans le QML. `build.rs` compile chaque
`translations/qml_<langue>.ts` en `.qm` et les place dans
`qrc:/qt/qml/AstroArchLocaleModule/qml/i18n/`, où `QQmlApplicationEngine` les charge
tout seul (langue du système au démarrage).

Le changement de langue en cours d'exécution se fait en QML avec `Qt.uiLanguage`
(voir `Main.qml`) : Qt charge `qml_<langue>.qm` et réévalue tous les `qsTr()`.
`qml_en.ts` (textes sources) permet de revenir à l'anglais ; toute langue sans
fichier `.ts` retombe sur `en`.

Ajouter une langue : créer `translations/qml_xx.ts` (copie d'un autre `.ts`,
traduire), puis ajouter `"xx"` à `translatedLanguages` dans `Main.qml`.

## Pré-remplissage et application des choix

Au lancement : langue du système, puis pays/clavier/langue selon l'IP si le
réseau est disponible. « Finish » demande une autorisation (pkexec) puis active
la locale dans `/etc/locale.gen`, lance `locale-gen`, applique `localectl
set-locale` et `localectl set-x11-keymap`, et met à jour `~/.config/plasma-localerc`.
Il faut fermer puis rouvrir la session pour que la nouvelle langue s'applique
aux applications déjà ouvertes (pas besoin de redémarrer).

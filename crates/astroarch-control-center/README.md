# astroarch-control-center

Centre de contrôle AstroArch : une fenêtre de boutons qui lancent les outils du système.

**100 % Rust** (interface en QML) : plus aucun fichier C++ ni CMake. Compilation avec Cargo seul.

## Compiler et installer

    cargo build --release
    sudo ./install.sh              # PREFIX=/usr par défaut

Prérequis : Qt 6 (Quick Controls 2), `qt6-tools` (pour `lrelease`, qui compile les
traductions ; sans lui l'interface reste en anglais), Konsole (ou un autre terminal),
zsh (pour `update-astroarch`).

## Boutons

| Bouton | Commande | Mode |
|--------|----------|------|
| Mise à jour du système | `update-astroarch` | console à part (zsh interactif : plugin zsh) |
| Langue et clavier | `astroarch-locale-module` | application |
| Services | `astroarch-sys-manager` | application |
| Sauvegarde | `astroarch-backup` | application |
| Moniteur astro | `astromonitor` | console à part |
| Logiciels | `astroarch-software-installer` | application |

Pour ajouter ou modifier un bouton : liste `entries` dans `qml/Main.qml`, rien à changer en Rust.
Terminaux essayés dans l'ordre : konsole, xfce4-terminal, lxterminal, gnome-terminal, kitty,
alacritty, foot, xterm.

## Traductions

`qsTr()` dans le QML, fichiers `translations/qml_<langue>.ts` (fr, de, es, it) compilés par
`build.rs` avec `lrelease` et chargés automatiquement selon la langue du système.
Après un changement de textes dans le QML :

    lupdate-qt6 qml -ts translations/qml_fr.ts translations/qml_de.ts translations/qml_es.ts translations/qml_it.ts

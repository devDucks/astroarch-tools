# astroarch-software-installer

Application 100 % Rust (CXX-Qt 0.10, Qt6 / QML, style KDE) pour AstroArch : affiche une liste de
logiciels lue dans un fichier TOML, avec un commutateur installer / désinstaller par ligne
et une courte description. Build uniquement avec `cargo` (ni CMake ni fichier C++).

> Projet généré sans compilation : les erreurs de build ou d'exécution sont à remonter pour correction.

## Arborescence

```
Cargo.toml  build.rs
src/main.rs        QApplication, style Breeze, chargement du QML
src/translator.rs  TranslatorManager : QTranslator écrit en Rust (charge le .qm du système)
src/manager.rs     objet QML PackageManager (pont vers backend)
src/backend.rs     TOML + pacman + pkexec (sans Qt)
qml/bootstrap.qml  installe la traduction avant l'interface
qml/main.qml       interface
i18n/*.ts          traductions fr / it / es / de (source : anglais)
data/software.toml liste d'exemple
org.astroarch.software-installer.desktop
```

`build.rs` compile les `.ts` en `.qm` avec `lrelease` (paquet `qt6-tools`) et les embarque dans
l'exécutable (`:/i18n/`). Si `lrelease` est introuvable, un avertissement s'affiche et l'interface
reste en anglais (variable `LRELEASE=/chemin/lrelease` pour l'indiquer à la main).

## Dépendances (Arch Linux ARM)

```
sudo pacman -S --needed base-devel rust qt6-base qt6-declarative qt6-tools qqc2-desktop-style polkit
```

## Compilation et lancement

```
cargo build --release
./target/release/astroarch-software-installer data/software.toml
# ou directement :
cargo run --release -- data/software.toml
```

## Installation sur le système

```
sudo install -Dm755 target/release/astroarch-software-installer /usr/bin/astroarch-software-installer
sudo install -Dm644 data/software.toml /usr/share/astroarch-software-installer/software.toml
sudo install -Dm644 org.astroarch.software-installer.desktop /usr/share/applications/org.astroarch.software-installer.desktop
```

## Fichier de liste

Ordre de recherche : 1er argument de la ligne de commande, variable `ASTROARCH_SOFTWARE_LIST`,
`/etc/astroarch/software.toml`, `/usr/share/astroarch-software-installer/software.toml`, `./data/software.toml`.

Champs de chaque `[[package]]` : `name` (obligatoire, nom du paquet pacman), `title`, `description`,
`category`, `aur` (facultatifs). La description affichée est celle du dépôt (`pacman -Si`) ; le champ
`description` du fichier (en anglais) ne sert que si le dépôt n'en fournit pas.

### Paquets AUR (paru)

Ajouter `aur = true` à l'entrée. Le logiciel est alors installé avec :

```
paru -S --needed --noconfirm --skipreview --nosudoloop --sudo pkexec <paquet>
```

`paru` tourne avec les droits de l'utilisateur (makepkg refuse root) et appelle `pkexec` pour les
étapes root (fenêtre polkit, éventuellement plusieurs fois : dépendances, puis installation).
Si `paru` n'est pas installé, le commutateur de ces entrées est désactivé. La désinstallation
reste `pkexec pacman -Rns`. Les paquets AUR n'ont pas de description de dépôt : celle du fichier
est utilisée. Une étiquette « AUR » s'affiche à côté du titre.

Attention : `--skipreview` compile le PKGBUILD sans relecture, donc n'ajoute à la liste que des
paquets AUR que tu as vérifiés. La compilation peut être longue sur le Raspberry Pi, sans barre de
progression (seulement l'indicateur d'activité).

## Fonctionnement

- État installé : `pacman -Qq` ; présence dans les dépôts : `pacman -Sl` (un logiciel absent des dépôts
  et non installé a son commutateur désactivé).
- Installation : `pkexec pacman -S --needed --noconfirm <paquet>` ; désinstallation :
  `pkexec pacman -Rns --noconfirm <paquet>`. Une seule opération à la fois.
- Les bases de synchronisation ne sont jamais rafraîchies par l'application (pas de `-Sy`).
- La langue suit celle du système (`LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`). Les catégories et
  titres du fichier ne sont pas traduits.

## Centre de contrôle

Le bouton 5 peut lancer simplement la commande `astroarch-software-installer`.

# astroarch-software-installer

A 100% Rust application (using CXX-Qt 0.10, Qt6/QML, and the KDE style) for AstroArch. It displays a list of software—read from a TOML file—with an install/uninstall toggle for each item, accompanied by a brief description. The project is built exclusively using `cargo` (no CMake or C++ source files).

## Directory Structure

```
Cargo.toml  build.rs
src/main.rs        QApplication, Breeze style, QML loading
src/translator.rs  TranslatorManager: A QTranslator implemented in Rust (loads system .qm files)
src/manager.rs     QML `PackageManager` object (bridge to the backend)
src/backend.rs     TOML parsing + `pacman` + `pkexec` (Qt-independent)
qml/bootstrap.qml  Loads translations before the interface
qml/main.qml       User interface
i18n/*.ts          Translations for FR/IT/ES/DE (source language: English)
data/software.toml Sample list
org.astroarch.software-installer.desktop
```

The `build.rs` script compiles `.ts` files into `.qm` files using `lrelease` (from the `qt6-tools` package) and embeds them into the executable (`:/i18n/`). If `lrelease` cannot be found, a warning is issued, and the interface will default to English (you can manually specify the path using the `LRELEASE=/path/to/lrelease` variable).

## Dependencies (Arch Linux ARM)

```
sudo pacman -S --needed base-devel rust qt6-base qt6-declarative qt6-tools qqc2-desktop-style polkit
```

## Compilation and execution

```
cargo build --release
./target/release/astroarch-software-installer data/software.toml
# ou directement :
cargo run --release -- data/software.toml
```

## System installation

```
sudo install -Dm755 target/release/astroarch-software-installer /usr/bin/astroarch-software-installer
sudo install -Dm644 data/software.toml /usr/share/astroarch-software-installer/software.toml
sudo install -Dm644 org.astroarch.software-installer.desktop /usr/share/applications/org.astroarch.software-installer.desktop
```

## List File

Search order: 1st command-line argument, variable `ASTROARCH_SOFTWARE_LIST`,
`/etc/astroarch/software.toml`, `/usr/share/astroarch-software-installer/software.toml`, `./data/software.toml`.

Fields for each `[[package]]`: `name` (required, pacman package name), `title`, `description`,

`category`, `aur` (optional). The displayed description is that of the repository (`pacman -Si`); the `description` field in the file (in English) is only used if the repository does not provide one.

### AUR Packages (published)

Add `aur = true` to the entry. The software is then installed with:

```
paru -S --needed --noconfirm --skipreview --nosudoloop --sudo pkexec <package>
```

`paru` runs with root privileges (makepkg refuses root access) and calls `pkexec` for the root steps (polkit window, possibly several times: dependencies, then installation).

If `paru` is not installed, the switch for these entries is disabled. Uninstallation is still done with `pkexec pacman -Rns`. AUR packages do not have a repository description: the one in the file is used. An "AUR" label is displayed next to the title.

Note: `--skipreview` compiles the PKGBUILD without rereading, so only add AUR packages that you have verified to the list. Compilation can be lengthy on the Raspberry Pi, without a progress bar (only an activity indicator).

## Operation

- Installed state: `pacman -Qq` ; Presence in repositories: `pacman -Sl` (software that is neither in the repositories nor installed will have its toggle disabled).
- Installation: `pkexec pacman -S --needed --noconfirm <package>`; uninstallation:
`pkexec pacman -Rns --noconfirm <package>`. Only one operation at a time.
- Synchronization databases are never refreshed by the application (no `-Sy`).
- The language follows the system settings (`LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`). Categories and
titles within the file are not translated.



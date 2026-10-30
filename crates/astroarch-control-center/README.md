# astroarch-control-center

AstroArch Control Center: the setup of a new AstroArch and, afterwards, its settings
and tools, in one window. **100 % Rust** with a QML interface, built with Cargo alone.

## What it shows

- **One list of pages** (`pages` in `qml/Main.qml`): network and remote access, language
  and time zone, passwords, update, location, equipment, data backup, kiosk, image
  storage, software, services, restore, Astro monitor, how to connect.
- **While the setup is shown**, the pages are grouped as Essentials, Recommended,
  Optional and More, with a status each (to do, done, set up later, not needed, on hold)
  and "Set up later" / "Continue". "I'm done, hide setup" keeps every page, grouped by
  topic instead (System, Observing, Data, Software and services, Connection).
- **Each page shows the current values** read from the system, one per row, with the
  buttons that change them (Plasma settings modules, KStars, PHD2..., or a view of the page
  itself). A page whose state can be read (network, location, equipment) is done as soon
  as the system says so.
- **One look for each task**: the tools of the other crates (services, software, backup,
  language and keyboard) are pages of this window, built from their own code. Their menu
  entries open the Control Center on that page alone (`--page ID --single`), so a task
  never looks different depending on where it was started.

## How it works

| File | Role |
|---|---|
| `qml/Main.qml` | The pages (data), the side panel, the page view |
| `scripts/astroarch-control-center-probe.sh` | Reads one piece of state (`probe NAME`): one line of output + exit code. Read-only, runs as the user. Installed as `/usr/lib/astroarch-control-center/astroarch-control-center-probe` |
| `src/setup_state.rs` | Runs the probes in the background (with a timeout); keeps the user's choices in `~/.config/astroarch/setup.json` |
| `src/launcher.rs` | Starts the tools (GUI or in a terminal), never twice, and tells when they exit so the page is read again |
| `qml/KioskSetup.qml` | The Kiosk page: profiles shared with the main desktop or separate, the apps the kiosk opens. Reads the kiosk with the `kiosk-*` probes and changes it with `pkexec /usr/lib/astroarch/astroarch-kiosk-setup apply …`, the helper of the astroarch repository (`scripts/astroarch-kiosk-setup.sh`, installed by `scripts/install-kiosk-setup.sh` with its polkit action); the apps are the lines of its `configs/kiosk/apps.list` |
| `qml/DataBackupView.qml`, `qml/SelectionList.qml`, `src/data_backup.rs` | The Data backup page: both profiles of the backup tool (SelectionList: the tick list both use). Its logic is compiled in from `crates/astroarch-backup` (`data_profile.rs`, `media.rs`, `storage.rs`, `config.rs`; `src/system_backup.rs` is a link to its `bridge.rs`): the same files, not a copy |
| `qml/SoftwareList.qml`, `src/software_manager.rs` | The More software page: the software installer's list and code (`software_manager.rs` is a link to its `manager.rs`) |
| `qml/LanguageView.qml`, `qml/PickList.qml`, `src/locale_bridge.rs` | The Language page's "Change…" and "Choose…": a wizard Country → Language → Keyboard → Apply with astroarch-locale-module's code (`locale_bridge.rs` is a link to its `bridge.rs`; `geolocation.rs`, `keyboard.rs`, `locale_gen.rs` are compiled in from that crate). See "Language view" below |
| `qml/ServicesView.qml`, `src/service_manager.rs` | The Services page: Bluetooth, file transfer (FTP) and USB power, read and switched by astroarch-sys-manager's code (`service_manager.rs` is a link to its `bridge.rs`); its state is polled only while the page is shown |
| `qml/StorageView.qml`, `src/storage_manager.rs` | Image storage page: moves the shared folder `/srv/astro-data` to a drive and back. The drive list is the backup tool's (`media.rs`, `storage.rs` of astroarch-backup, compiled in) |
| `qml/RestoreView.qml`, `src/restore.rs` | The Restore page: KStars, INDI, PHD2 profiles and settings (and images) from a data backup snapshot, a system backup snapshot, an old AstroArch disk or AstroMonitor. What it offers is the data backup's catalog and plan (`data_profile.rs`, read in the backup instead of the system); the places and the network folder form are the Data backup page's (`DataBackup`, `ShareForm.qml`) |
| `scripts/astroarch-restore-helper.sh` | The Restore page's code run as root (pkexec, `scripts/org.astroarch.control-center.restore.policy`): mounts an old disk read-only, and copies: read as root only from a read-only mount made by root (the old disk), anything else as the main user; the user (or its kiosk user) writes; what was there is moved aside to `~/.local/share/astroarch/restore-DATE` and can be put back. Installed as `/usr/lib/astroarch-control-center/astroarch-restore-helper` |
| `scripts/astroarch-storage-helper.sh` | The Image storage page's code run as root (pkexec, `scripts/org.astroarch.control-center.storage.policy`): copies the folder, writes its `/etc/fstab` entry and udev rule. Installed as `/usr/lib/astroarch-control-center/astroarch-storage-helper`, the policy in `/usr/share/polkit-1/actions` |

To add a page or a row: add it to `pages`; if it needs a value from the system, add a
case to the probe script. Nothing to change in Rust. To offer something new in the data
backup: an entry in `crates/astroarch-backup/data/data-profile.toml`.

### Language view

The rows of the Language page start one wizard at the step they name: Language and
Keyboard "Change…" at those steps, "Country of your network" at the first step with that
country chosen (the probe `country-suggest` finds it only while language, keyboard and time
zone are still the image defaults; the view makes no lookup of its own). A country chooses
its language and keyboard layout (the locale module's `set_country`), checked in the next
steps. The lists are those of the module: the locales of `/etc/locale.gen`, the layouts and
variants of `/usr/share/X11/xkb/rules/base.lst`.

Nothing changes before **Apply**, which is the module's `persist_configuration`: first the
locale enabled in `/etc/locale.gen`, `locale-gen` and `localectl set-locale` in one
`pkexec`, and `~/.config/plasma-localerc`; only if that worked, `localectl set-x11-keymap`
(polkit), then the session layout (`kxkbrc` on Wayland). A refused password changes no
keyboard. The keyboard step's test field uses the session layout; **Try this layout** sets
the chosen one in the session only (`setxkbmap` on X11, `kxkbrc` on Wayland). The module
saved the whole session layout first (several layouts, their switch option, kxkbrc's
display names) and puts it back on Cancel, when another page is shown, when Apply fails,
or when the window closes. The module's own IP lookup is its cargo feature `geoip`
(reqwest), off here: the country comes from the probe. The time
zone rows stay as they were (KDE's clock settings, and "Use" for the zone of the network).

### Image storage on a drive

The folder goes to `<drive>/astroarch/astro-data` and is mounted at `/srv/astro-data` by
an `/etc/fstab` entry (by UUID, `X-mount.subdir`, `nofail`: AstroArch starts without the
drive), under a `# astroarch-storage: <drive name>` line. A udev rule
(`/etc/udev/rules.d/90-astroarch-storage.rules`) mounts it again when the drive is plugged
back in. Without the drive, `/srv/astro-data` is an empty folder that nobody can write in
(`chattr +i`, with a `DRIVE-NOT-CONNECTED.txt` note): programs get an error instead of
filling the internal disk. On ext4, Btrfs, XFS, F2FS the files keep their owners and the
folder its group `astro` and default ACL; exFAT, FAT32 and NTFS are mounted with group
`astro` and group-writable permissions. Nothing is ever formatted. Test it on another
folder with `sudo ASTRO_DATA_DIR=/some/folder scripts/astroarch-storage-helper.sh ...`
(the probes read `ASTRO_DATA_DIR` too).

### Restore

Each source is a folder holding the files of the old system at their absolute paths
(`<root>/home/astronaut/.indi`): a snapshot of the data backup (`<id>/settings/snapshot-DATE`
and `<id>/data`), the old disk mounted under `/run/astroarch-cc-restore` (`ro,noload`; the
device itself is set read-only too when nothing else, e.g. the file manager, has it mounted), or the AstroMonitor backup, downloaded as its own "Restore Backup"
does (`GET http://astromatto.com:11111/backup/db/<token>`, a tar archive) and laid out in
`~/.cache/astroarch/restore-astromonitor`. A snapshot of the **system backup** (the backup
tool's btrfs image) is a source too: the page finds it where the Data backup page's System
profile does (`~/.backup_dest`, `/var/backups`, the drives with `astroarch/backups`), mounts
it read-only with the backup helper's `mount --dest DIR` (`BackupManager.browse`, released
with `umount` when the page leaves the source or closes) and reads
`/run/astroarch-backup/browse/snapshots/<name>`. A snapshot made on another architecture (a
Raspberry Pi's on a PC) cannot be restored whole by the backup tool (its helper refuses it,
error 44); here it is offered like any other, with "settings and data only". Only the
catalog's paths are copied, never system files: the restore helper reads only under a
root's `home/` and `srv/astro-data`, and under `/run` accepts only an old disk it mounted or
a snapshot of the browse mount. Each source is resolved once when checked and only that
path is used (its folder is entered and checked again before reading; links inside are
copied as links). Root reads only from the read-only mounts made by root (an old disk whose
device it set read-only, a browsed snapshot, held open during the restore); anything else
(a data backup, the AstroMonitor download) is read as the main user. The old user's home becomes the new user's, `<user>-kiosk` the new kiosk's; kiosk
profiles that are links to the main user's here (shared on the Kiosk page) are left alone,
and the kiosk helper shares the restored ones again. KStars, PHD2 and indiserver must be
closed. Test the helper on other users and folders with `sudo ASTROARCH_MAIN_USER=u
ASTROARCH_KIOSK_USER=u-kiosk ASTRO_DATA_DIR=/some/folder scripts/astroarch-restore-helper.sh ...`.

## Build and run

    cargo build --release -p astroarch-control-center
    target/release/astroarch-control-center
    target/release/astroarch-control-center --page kiosk    # opens on one page (its id in `pages`)
    target/release/astroarch-control-center --autostart     # at login: opens only while the setup is shown
    target/release/astroarch-control-center --page services --single   # that page alone

`--single` (with `--page`) shows only that page: no side panel, no setup progress and no
setup buttons ("Set up later", "Continue"...), a smaller window titled with the page.
The page's own actions and views work as usual; the setup state is not changed. The menu
entries of the other tools use it: `--page services` (astroarch-sys-manager),
`--page software` (astroarch-software-installer), `--page backup` (astroarch-backup),
`--page language` (astroarch-locale-module). With --single on one of those pages the
window takes that menu entry's app id (src/main.rs), so the task manager shows it with
its own name and icon.

Icons: `org.astroarch.<tool>` from the icon theme. They live in devDucks/astroarch-assets
(`AstroArch icons/control-center_icons/`), and the package installs the full drawings as
`scalable` and the simplified `sm_` ones for 16, 22 and 32 px.

Requirements: Qt 6 (Quick, Quick Controls 2), Kirigami, `qt6-tools` (lrelease, for the
translations). The probes use `nmcli`, `sqlite3`, `timedatectl`, `curl`, `smbclient` and
pam's `unix_chkpwd`; the image storage helper `rsync`, `chattr` and `setfacl`; the restore
helper `tar`, `runuser` and `blockdev`. A development build finds the other tools next to its
executable, and the probe script and the storage and restore helpers in this crate (without
their polkit policies, pkexec then asks for an administrator password each time).

Installed, the Data backup, Image storage and Restore pages also need their root helpers and polkit
policies:

- **Data backup**: from `crates/astroarch-backup`, `scripts/astroarch-backup-helper.sh` as
  `/usr/lib/astroarch-backup/astroarch-backup-helper`, `scripts/org.astroarch.backup.policy`
  in `/usr/share/polkit-1/actions`, and `systemd/astroarch-data-backup.service` and
  `.timer` in `/usr/lib/systemd/user` (the backup every 2 hours).
- **Image storage**: from this crate, `scripts/astroarch-storage-helper.sh` as
  `/usr/lib/astroarch-control-center/astroarch-storage-helper` and
  `scripts/org.astroarch.control-center.storage.policy` in `/usr/share/polkit-1/actions`.
- **Restore**: from this crate, `scripts/astroarch-restore-helper.sh` as
  `/usr/lib/astroarch-control-center/astroarch-restore-helper` and
  `scripts/org.astroarch.control-center.restore.policy` in `/usr/share/polkit-1/actions`.

## Translations

`qsTr()` in the QML, `translations/qml_<language>.ts` (fr, de, es, it) compiled by
`build.rs` with `lrelease` and loaded according to the system language. After changing
texts in the QML:

    lupdate qml -ts translations/qml_fr.ts translations/qml_de.ts translations/qml_es.ts translations/qml_it.ts
    lupdate -pluralonly qml -ts translations/qml_en.ts

`qml_en.ts` holds only the English plurals (`%n profile(s)` → "1 profile", "2 profiles").

## Images

See `resources/CREDITS.md`.

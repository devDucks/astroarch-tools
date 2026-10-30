# astroarch-control-center

AstroArch Control Center: the setup of a new AstroArch and, afterwards, its settings
and tools, in one window. **100 % Rust** with a QML interface, built with Cargo alone.

## What it shows

- **One list of pages** (`pages` in `qml/Main.qml`): network and remote access, language
  and time zone, passwords, update, location, equipment, data backup, kiosk, image
  storage, software, services, Astro monitor, how to connect.
- **While the setup is shown**, the pages are grouped as Essentials, Recommended,
  Optional and More, with a status each (to do, done, set up later, not needed, on hold)
  and "Set up later" / "Continue". "I'm done, hide setup" keeps every page, grouped by
  topic instead (System, Observing, Data, Software and services, Connection).
- **Each page shows the current values** read from the system, one per row, with the
  buttons that change them (Plasma settings modules, the other AstroArch tools, KStars,
  PHD2...). A page whose state can be read (network, location, equipment) is done as soon
  as the system says so.

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
| `qml/ServicesView.qml`, `src/service_manager.rs` | The Services page: Bluetooth, file transfer (FTP) and USB power, read and switched by astroarch-sys-manager's code (`service_manager.rs` is a link to its `bridge.rs`); its state is polled only while the page is shown |
| `qml/StorageView.qml`, `src/storage_manager.rs` | Image storage page: moves the shared folder `/srv/astro-data` to a drive and back. The drive list is the backup tool's (`media.rs`, `storage.rs` of astroarch-backup, compiled in) |
| `scripts/astroarch-storage-helper.sh` | The Image storage page's code run as root (pkexec, `scripts/org.astroarch.control-center.storage.policy`): copies the folder, writes its `/etc/fstab` entry and udev rule. Installed as `/usr/lib/astroarch-control-center/astroarch-storage-helper`, the policy in `/usr/share/polkit-1/actions` |

To add a page or a row: add it to `pages`; if it needs a value from the system, add a
case to the probe script. Nothing to change in Rust. To offer something new in the data
backup: an entry in `crates/astroarch-backup/data/data-profile.toml`.

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

## Build and run

    cargo build --release -p astroarch-control-center
    target/release/astroarch-control-center
    target/release/astroarch-control-center --page kiosk    # opens on one page (its id in `pages`)

Requirements: Qt 6 (Quick, Quick Controls 2), Kirigami, `qt6-tools` (lrelease, for the
translations). The probes use `nmcli`, `sqlite3`, `timedatectl`, `curl`, `smbclient` and
pam's `unix_chkpwd`; the image storage helper `rsync`, `chattr` and `setfacl`. A development
build finds the other tools next to its executable, and the probe script and the storage
helper in this crate (without its polkit policy, pkexec then asks for an administrator
password each time).

Installed, the Data backup and Image storage pages also need their root helpers and polkit
policies:

- **Data backup**: from `crates/astroarch-backup`, `scripts/astroarch-backup-helper.sh` as
  `/usr/lib/astroarch-backup/astroarch-backup-helper`, `scripts/org.astroarch.backup.policy`
  in `/usr/share/polkit-1/actions`, and `systemd/astroarch-data-backup.service` and
  `.timer` in `/usr/lib/systemd/user` (the backup every 2 hours).
- **Image storage**: from this crate, `scripts/astroarch-storage-helper.sh` as
  `/usr/lib/astroarch-control-center/astroarch-storage-helper` and
  `scripts/org.astroarch.control-center.storage.policy` in `/usr/share/polkit-1/actions`.

## Translations

`qsTr()` in the QML, `translations/qml_<language>.ts` (fr, de, es, it) compiled by
`build.rs` with `lrelease` and loaded according to the system language. After changing
texts in the QML:

    lupdate qml -ts translations/qml_fr.ts translations/qml_de.ts translations/qml_es.ts translations/qml_it.ts

## Images

See `resources/CREDITS.md`.

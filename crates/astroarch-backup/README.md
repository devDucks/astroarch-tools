# astroarch-backup

AstroArch backup assistant. Backups are stored in a
**pre-allocated Btrfs image file** (`astroarch-backup.btrfs`) to protect them
from corruption and accidental mishandling.

**100% Rust** (with a QML interface): no remaining C++ files. Compiled using only Cargo.

## Build and install

cargo build --release
sudo ./install.sh              # Defaults to PREFIX=/usr

Prerequisites: Qt 6 (Quick Controls 2), `qt6-tools` (for `lrelease`, which compiles
translations; without it, the interface remains in English), `rsync`, `btrfs-progs`, `polkit`.
If not installed, the application locates `scripts/astroarch-backup-helper.sh`
relative to the `target/` directory (use the `ASTROARCH_BACKUP_HELPER` variable
to specify a custom path).

## Wizard Workflow

1. **Folders to Exclude** — An external window displays the directory tree of your home folder and, if present, the shared data directory `/srv/astro-data` (including subfolders and their sizes; hidden folders can also be shown). Checking a box excludes that folder and all its contents. Previously excluded folders are remembered and suggested again. Certain essential folders cannot be excluded (🔒): `Desktop`, `.astroarch`, `.config`, `.local`, and `.oh-my-zsh` within the home folder, along with their contents (the helper will also reject these). If `/srv/astro-data` is a separate mounted file system, `rsync` (using the `-x` flag) will not back it up in any case.
2. **Analysis** — Calculates the data to be backed up (the entire `/` system, minus any exclusions) and checks the available space in `/var/backups`.
3. **Location**
- Main disk: **`/var/backups` (fixed)** (per FHS standards; cannot be changed). If there is sufficient space, the user can choose between "Save to main disk" and "Use an external disk." 
- If there is insufficient space: an external disk is **required**.
4. **External Disk** — Displays a list (automatically updated) of all file systems not located on the system disk, with a "Mount" button (via `udisks`) for those that are not currently mounted. For a suitable disk with enough available space, the application will ask for confirmation to create the directory `<disk>/astroarch/backups`. If there is not enough space, you will be prompted to select a different disk. FAT32-formatted drives are not supported (due to the 4 GB file size limit). 5. **Ready** — summary and **number of backups to keep** (1 to 50,
saved), then start.

You can **quit** at any time (using the button or the window's close icon). If you quit while a backup is in progress, the process is cancelled: the helper automatically completes its cleanup, and existing backups remain untouched.

## Full System Restore

From the main page: "Restore a backup..." → select backup location
(main disk or external drive) → choose snapshot → warning + confirmation
(enter `RESTORE`) → restoration performed **on the running system**, without requiring a separate system.

Principle (lightweight "takeover" using a `restore` helper):

1. **Checks (non-destructive)**: verify the image and snapshot (must include `/boot`),
check **PARTUUID/UUID** (the `root=` parameter in the snapshot's `cmdline.txt` must match the current root partition; otherwise, the Raspberry Pi would fail to boot, so the operation is blocked), and ensure all disks referenced in the snapshot's `fstab` are present (unless marked `nofail` or `noauto`).
2. **Safety backup** of the current state (enabled by default): creates an additional snapshot—excluded from the rotation policy—allowing the restore operation itself to be undone.
3. Essential tools (bash, rsync, mount, losetup, etc.) are copied—**along with their required libraries**—into RAM (`/run/astroarch-restore/root`, acting as a chroot environment). This ensures they remain functional even as files on the disk are modified. A preliminary test runs these tools before any changes are made.
4. A temporary systemd service stops the desktop environment and nearly all other services (`systemctl isolate`), then executes the following within the chroot: `rsync --delete-after` to sync the snapshot to the root partition, followed by syncing the snapshot's `boot/` directory to the system's `/boot` partition.
5. Immediate reboot (via sysrq). Progress is displayed in text mode (tty1).

Consequences (communicated to the user):
- Any changes made since the snapshot was taken—including new files, installed packages, updates, and configuration settings—will be lost. This is intentional, as it allows for rolling back a failed update. - Directories excluded from backups are **emptied** (they remain, but empty, within the snapshot). 
Separately mounted drives (e.g., `/srv/astro-data`), `/var/backups`, and the contents of
`/dev`, `/proc`, `/sys`, `/run`, `/tmp`, `/mnt`, and `/media` are never affected.
- A power failure or manual shutdown during the restoration process may render the
system unusable; this risk is assumed by the user (a warning is provided before starting).
- Only snapshots created **with** a copy of `/boot` can be restored.
- The log is copied to `/var/backups/astroarch-restore-<date>.log` when
backups are stored on the main disk.

## "Data and settings" profile (data backup)

The tool's second profile, next to the system backup: the settings of KStars, PHD2, INDI
and the kiosk, plus `/srv/astro-data`, **every 2 hours**, without root. It is set up and
started from the Control Center's "Data backup" page (no window in this app); the logic
is here and the Control Center compiles the same files.

- **What is offered**: `data/data-profile.toml` (one entry per line of the list: name,
  paths, `settings` or `data`, essential or not). Adding an item = one entry, no code. The
  user keeps **exclusions** (`~/.config/astroarch/data-backup.json`), so a new folder is
  included by itself. The KStars dark library keeps `userdb.sqlite` (the list of darks is
  in it).
- **Backup**: `astroarch-backup --data-backup` (no window), started by
  `systemd/astroarch-data-backup.timer` (user unit, `/usr/lib/systemd/user/`) or by "Back
  up now" (`--manual`). Skipped while KStars and indiserver run (a session in progress,
  unless `--manual`), one at a time (lock), never creates the base folder (a disconnected
  drive is an error, not a copy on the SD card).
- **In the destination** `<folder>/<system id>/`: `settings/snapshot-YYYYMMDD-HHMMSS/`
  (history, unchanged files as hard links, the last N kept) and `data/` (images: only
  added). Full paths inside (`rsync -R`).
- **Destinations**: this disk (`/var/backups/astroarch-data/<user>`, outside the system
  backup), a USB drive (`<drive>/astroarch/data-backup`, not FAT/exFAT/NTFS: no permissions,
  no hard links), a network share already mounted, or Syncthing when installed (send-only
  folder on the Pi, the versions on the computer; a single copy `settings/current/`). The
  helper creates the folder once: `datadir --dest DIR`.
- **Network share** added from the page ("Connect a network folder", SMB or NFS): helper
  `nas-add` (password on stdin, credentials in `/etc/astroarch-backup/nas-<id>.cred`, 0600
  root, an `/etc/fstab` line after `# astroarch-backup nas <id>`, mounted at
  `/mnt/astroarch-nas/<id>` on first use, `nofail,_netdev,x-systemd.automount`: the Pi
  starts without the NAS). The share is mounted once to check it before anything is
  written. `nas-remove --id <id>` removes everything. On a share, rsync compares times and
  sizes only (`-rlt`): the hard links of the snapshots work despite the permissions the
  mount imposes.
- The page also chooses the **folders excluded from the system backup** with the same
  list (folders of the home folder and of `/srv/astro-data`, the system ones locked),
  stored in `~/.backup_dest` as before.
- State of the last backup: `~/.local/state/astroarch/data-backup.json`; log:
  `journalctl --user -u astroarch-data-backup`.

## How it provides protection

- **Btrfs image**: Uses checksums for all data and a copy-on-write mechanism.
- **Read-only snapshots** (`snapshots/YYYY-MM-DD_HHMMSS`) are created only after a successful `rsync` operation; an interrupted, failed, or canceled backup will never affect existing snapshots. Only the last *N* snapshots are retained (where *N* is configurable).
- **Verification** (`btrfs scrub`) is performed after every backup; older snapshots are deleted only if the verification succeeds.
- **When not in use**, the image is unmounted and set to be immutable (`chattr +i`); even the `root` user must remove this attribute before any changes can be made.
- **Boot partitions**: `rsync` runs with the `-x` flag (staying within a single file system), so the `/boot` partition (FAT32) is not traversed. Boot partitions mounted separately—such as `/boot`, `/boot/efi`, or `/boot/firmware`—are copied in a second pass into directories like `work/boot`. Consequently, they are included in every snapshot and factored into disk space calculations. If `/boot` is part of the root partition (`/`), it is already included. Since the FAT file system does not support ACLs or extended attributes, these are not copied for those partitions.
- `rsync` is executed with `--inplace --no-whole-file`, ensuring that when a file is modified, only the changed blocks are rewritten (otherwise, each snapshot would store a complete copy of the file).
- **Limitation**: When using the `single` data profile, Btrfs can *detect* corruption but cannot *repair* it (refer to `DATA_PROFILE` in the helper script; using `dup` allows for repairs, but it doubles the required disk space). Furthermore, an image stored on the primary disk does not provide protection against a failure of that disk.
- **Prerequisites**: Requires `btrfs-progs` and the `btrfs` kernel module.

## Image file size

- At creation: data (excluding excluded directories) + 20% + 2 GiB (minimum 4 GiB), not accounting for zstd compression.
- Thereafter, the size tracks ACTUAL usage: before each copy operation, the helper calculates the amount of data to be added (via a dry-run `rsync`) and expands the image only if the available internal free space (factoring in compression) is insufficient.
- **Automatic shrinking**: following a successful and verified backup, if the file is at least 1 GiB in size AND has an excess margin of 15% or more, it is shrunk to "used space + 15% + 1 GiB" and then re-verified. A failure during the shrinking process will never cause the backup itself to fail.
- Older snapshots retain previous versions of modified files: the total space used increases as snapshots accumulate, up to a certain number (N), after which it stabilizes.

## Structure

```
Cargo.toml           Rust dependencies (cxx-qt 0.10, serde, serde_json, thiserror)
build.rs             cxx-qt bridge + QML module + translation compilation (lrelease)
install.sh           installation (binary, helper, polkit, launcher)
org.astroarch.backup.desktop   launcher (icon, application name)
src/
main.rs            startup: QGuiApplication + loading Main.qml
bridge.rs          BackupManager QObject exposed to QML (invokes the helper)
storage.rs         location checks, space estimation, helper interaction
media.rs           external drive detection (lsblk) and mounting (udisks)
disk_analysis.rs   calculating the size of data to back up (via the helper)
config.rs          configuration (last location, retention policy)
data_profile.rs    "Data and settings" profile: catalog, choices, backup
data/
data-profile.toml  what the data backup offers
systemd/
astroarch-data-backup.service, .timer   user units (every 2 hours)
scripts/
astroarch-backup-helper.sh  the ONLY code executed as root (via pkexec)
org.astroarch.backup.policy Polkit action (password remains valid for a few minutes)
qml/
Main.qml               wizard (5 screens) and dialogs
FolderTreeWindow.qml   external window: directory tree with checkboxes
translations/
qml_fr.ts, qml_it.ts, qml_es.ts, qml_de.ts
```

## Helper root

`astroarch-backup-helper backup|measure|shrink|explain|mount|umount|usage|prune|snapshots|restore ...`
It validates all its arguments (it constructs the `rsync` command itself; the application never passes `rsync` options to it). Excluded directories must be absolute paths located within the caller's home directory or under `/srv/astro-data`, containing neither `..` nor `rsync` patterns.
`/var/backups` is never backed up (an image may reside there). Cancellation is handled via a flag file (since the helper runs as root, the application cannot simply kill the process). For the output protocol and error codes, refer to the script header.

The helper is installed at a fixed path (`/usr/lib/astroarch-backup/`), which is referenced by the Polkit file.

## Viewing the contents of the backup

The image is not visible while it is unmounted. To inspect the contents (the directory is `/var/backups` or `<disk>/astroarch/backups`):

H=/usr/lib/astroarch-backup/astroarch-backup-helper   # or scripts/astroarch-backup-helper.sh
pkexec $H explain --dest /var/backups [--exclude PATH]...   # shows what takes up space, sorted by size
pkexec $H mount   --dest /var/backups      # mounts in READ-ONLY mode at /run/astroarch-backup/browse
ls /run/astroarch-backup/browse/snapshots  # one directory per backup
pkexec $H umount  --dest /var/backups
pkexec $H usage   --dest /var/backups      # space used + share of each snapshot
pkexec $H prune   --dest /var/backups --keep 2   # keep only 2 snapshots (IRREVERSIBLE), then shrink the file
pkexec $H shrink  --dest /var/backups      # shrink on demand

While a backup is mounted for inspection, any new backup or shrinking operation will be rejected (you must `umount` first).

## Translations

Text in the `.qml` files is written in English using `qsTr("...")`. No extra code is required to load it: `QQmlApplicationEngine` automatically loads the appropriate `qml_<language>.qm` file from the `i18n` directory located next to `Main.qml`. The `build.rs` script uses `lrelease` to compile the `translations/qml_<language>.ts` files and place them in that directory. The language defaults to the system language (`LANG`), with English as a fallback.
To update: `lupdate qml/*.qml -ts translations/qml_fr.ts` (repeat for `it`, `es`, `de`).

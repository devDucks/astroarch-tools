#!/bin/bash
# astroarch-backup-helper
#
# Run as root (via pkexec) by the astroarch-backup application. This is the
# ONLY place that interacts with the disk: the graphical interface does not perform any
# privileged operations itself.
#
# How it works: backups are stored in a PREALLOCATED Btrfs image file
#   <destination>/astroarch-backup.btrfs
# which is mounted (via loop) only during the backup, then protected by
# the immutable attribute (chattr +i). Inside:
#   work/            a mirror of / updated by rsync
#   snapshots/<date> READ-ONLY snapshots of work/
#
# Separately mounted boot partitions (e.g., /boot on FAT32, /boot/efi,
# /boot/firmware) are not processed by rsync -x: they are copied separately,
# into work/<mount point>, and are therefore present in every snapshot.
#
# After a successful AND verified backup, if the image file is significantly
# larger than necessary (at least 1 Gio and a 15% margin, for
# example due to compression), it is automatically reduced to
# “used space + 15% + 1 Gio.” A failure during reduction never causes a blockage.
#
# Guarantees:
#   - An interrupted, failed, or canceled backup NEVER affects existing snapshots
#     (Btrfs is copy-on-write; the snapshot is created only after a
#     successful rsync);
#   - Btrfs checksums all data; a `scrub` is run after each
#     backup, and old snapshots are deleted only if it succeeds;
#   - Outside of a backup, the image is unmounted and immutable: even the root user must first
#     remove the attribute (chattr -i) to modify or delete it.
#
# Subcommands:
#   measure --dest DIR [--exclude PATH]...   (PATH: absolute, in your home directory or /srv/astro-data)
#   backup  --dest DIR --size BYTES --cancel-file FILE [--keep N] [--exclude PATH]...
#   shrink  --dest DIR [--size BYTES|auto]
#   explain --dest DIR [--exclude PATH]...   (what the backup would contain, by size)
#   mount   --dest DIR                      (browse the backup, READ-ONLY)
#   umount  --dest DIR
#   usage   --dest DIR                      (space used, share of each snapshot)
#   prune   --dest DIR --keep N             (keep only the last N snapshots; IRREVERSIBLE)
#   snapshots --dest DIR                    (list of snapshots: SNAPSHOT:<name>:<1 if /boot is present>)
#   restore --dest DIR --snapshot NAME --confirm RESTORE
#                                         (RESTAURATION COMPLÈTE du système en marche, voir plus bas)
#   nas-add --kind smb|nfs --server HOST --share NAME|/EXPORT [--user NAME]
#                                         (password on stdin: a network share for the data
#                                          backup, mounted when used; see cmd_nas_add)
#   nas-remove --id ID                      (forget a share added by nas-add)
#   datadir --dest DIR                      (folder of the "Data and settings" backup, owned
#                                            by the caller: see cmd_datadir)
#
# Output protocol (stdout, one piece of information per line; rsync passes it through as-is):
#   STAGE:<preparing|copying|boot|snapshot|verifying|pruning|shrinking|finishing>
#     (the app's progress bar follows rsync up to 99%; it stays at 99% during
#      boot/snapshot/verifying/pruning/shrinking and jumps to 100% at “finishing”)
#   WARN:<message>
#   RESULT:size:<bytes>        (measure)
#   RESULT:ok:<snapshot name> (backup)
#   RESULT:size:<bytes>        (shrink: new image file size)
#   RESULT:unchanged:<bytes>   (shrink: nothing to shrink)
#   ERROR:<code>:<message>
#
# Exit codes: 0 OK, 10 arguments, 11 insufficient space, 12
# unsuitable location, 20 rsync failure, 21 verification failed, 30 already in progress,
# 40 image cannot be mounted, 41 preparation impossible, 130 canceled.

set -Eeuo pipefail
export LC_ALL=C
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin
umask 077

readonly IMG_NAME="astroarch-backup.btrfs"
readonly MARKER_NAME="astroarch-backup.incomplete"
readonly RUN_DIR="/run/astroarch-backup"
readonly MNT="$RUN_DIR/mnt"
readonly LOCK_FILE="$RUN_DIR/lock"
readonly BROWSE="$RUN_DIR/browse"
readonly KEEP_SNAPSHOTS=5
# nodiscard: A discard on a loop “pierces” the preallocated file (and makes it empty again)
readonly MOUNT_OPTS="compress=zstd:3,noatime,nodiscard"
readonly HOST_MARGIN=$((128 * 1024 * 1024))
# Data profile: “single” (detects corruption using checksums).
# “dup” would store two copies and allow scrub to REPAIR, but it doubles
# the space required (the application would then need to double the size of the image)
readonly DATA_PROFILE="single"

# Files in the personal folder that are essential for proper operation: they (and their
# contents) must NOT be excluded from the backup
readonly PROTECTED_HOME_DIRS=(Desktop .astroarch .config .local .oh-my-zsh)

LAST_ERR=""
PENDING_TRUNCATE=""
LOOP_DEV=""
MOUNTED=0
CANCELED=0
FIRST_RUN=0
WATCH_PID=""
RSYNC_PID=""
IMG=""
MARKER=""
DEST=""
CALLER_UID=""
CALLER_HOME=""
EXCLUDES=()
EMPTY_DIR=""

say()   { printf '%s\n' "$*"; }
stage() { say "STAGE:$1"; }
warn()  { say "WARN:$*"; }
fail()  { local code=$1; shift; say "ERROR:$code:$*"; exit "$code"; }

is_uint() { [[ "$1" =~ ^[0-9]+$ ]]; }

# Order cancellation -> one line only (300 characters max)
oneline() { printf '%s' "$1" | tr '\n\r' '  ' | tr -s ' ' | cut -c1-300; }

# ABSOLUTE path accepted for exclusion: within the caller's home directory
# or the shared data directory /srv/astro-data; no “..”, no
# empty components, no rsync patterns
safe_excl_path() {
    local p=$1 c
    [[ "$p" == /* && "$p" != */ ]] || return 1
    case "$p" in
        *'*'*|*'?'*|*'['*|*']'*|*'\'*|*$'\n'*) return 1 ;;
    esac
    case "$p" in
        "$CALLER_HOME"/*|/srv/astro-data|/srv/astro-data/*) ;;
        *) return 1 ;;
    esac
    local IFS=/
    for c in ${p#/}; do
        [[ -n "$c" && "$c" != . && "$c" != .. ]] || return 1
    done
    # Required Document (Code 2: Dedicated Message)
    local d
    for d in "${PROTECTED_HOME_DIRS[@]}"; do
        case "$p" in
            "$CALLER_HOME/$d"|"$CALLER_HOME/$d"/*) return 2 ;;
        esac
    done
    return 0
}

protect_image() {
    if [[ -n "$IMG" && -f "$IMG" ]]; then
        chmod 600 -- "$IMG" 2>/dev/null || true
        chattr +i -- "$IMG" 2>/dev/null || warn "immutable-flag-unsupported"
    fi
}

unprotect_image() {
    if [[ -n "$IMG" && -f "$IMG" ]]; then
        chattr -i -- "$IMG" 2>/dev/null || true
    fi
}

cleanup() {
    local rc=$?
    set +e
    if [[ -n "$WATCH_PID" ]]; then kill "$WATCH_PID" 2>/dev/null; fi
    # Stopped by a signal during the copy: stop rsync before unmounting, or it would
    # keep writing into an image that is no longer protected
    if [[ -n "$RSYNC_PID" ]]; then kill -TERM "$RSYNC_PID" 2>/dev/null; wait "$RSYNC_PID" 2>/dev/null; RSYNC_PID=""; fi
    if (( MOUNTED )); then
        sync
        if umount -- "$MNT" 2>/dev/null; then
            MOUNTED=0
        else
            sleep 1
            if umount -- "$MNT" 2>/dev/null; then MOUNTED=0; else umount -l -- "$MNT" 2>/dev/null; fi
        fi
    fi
    if [[ -n "$LOOP_DEV" ]]; then
        if losetup -d "$LOOP_DEV" 2>/dev/null; then LOOP_DEV=""; fi
    fi
    rmdir -- "$MNT" 2>/dev/null
    # File reduction: only if the file system is properly
    # unmounted and the loop device detached (never in the case of a “lazy” unmount)
    if [[ -n "$PENDING_TRUNCATE" && $MOUNTED -eq 0 && -z "$LOOP_DEV" && -f "$IMG" ]]; then
        truncate -s "$PENDING_TRUNCATE" -- "$IMG" 2>/dev/null
    fi
    if [[ -n "$IMG" && -e "$IMG" ]]; then
        if (( CANCELED )) && [[ -e "$MARKER" ]]; then
            # Canceling a FIRST backup: incomplete file, with
            # no snapshots → it is deleted
            rm -f -- "$IMG" "$MARKER"
        else
            protect_image
        fi
    fi
    if (( CANCELED )); then rc=130; fi
    exit "$rc"
}

# ---------------------------------------------------------------- arguments
parse_args() {
    local SIZE_ARG=""
    CANCEL_FILE=""
    KEEP=""
    SNAPSHOT=""
    CONFIRM=""
    NAS_KIND=""
    NAS_SERVER=""
    NAS_SHARE=""
    NAS_USER=""
    NAS_ID=""
    while (( $# )); do
        (( $# >= 2 )) || fail 10 "missing value for $1"
        case "$1" in
            --dest)        DEST=$2 ;;
            --size)        SIZE_ARG=$2 ;;
            --cancel-file) CANCEL_FILE=$2 ;;
            --keep)        KEEP=$2 ;;
            --snapshot)    SNAPSHOT=$2 ;;
            --confirm)     CONFIRM=$2 ;;
            --exclude)     EXCLUDES+=("$2") ;;
            --kind)        NAS_KIND=$2 ;;
            --server)      NAS_SERVER=$2 ;;
            --share)       NAS_SHARE=$2 ;;
            --user)        NAS_USER=$2 ;;
            --id)          NAS_ID=$2 ;;
            *)             fail 10 "unknown option: $1" ;;
        esac
        shift 2
    done
    SIZE=$SIZE_ARG
}

# Number of snapshots to keep: --keep, otherwise KEEP_SNAPSHOTS
resolve_keep() {
    if [[ -z "${KEEP:-}" ]]; then KEEP=$KEEP_SNAPSHOTS; fi
    is_uint "$KEEP" || fail 10 "invalid --keep"
    (( KEEP >= 1 && KEEP <= 100 )) || fail 10 "--keep must be between 1 and 100"
}

validate_common() {
    (( EUID == 0 )) || fail 10 "must run as root"
    CALLER_UID=${PKEXEC_UID:-}
    is_uint "$CALLER_UID" || fail 10 "must be started through pkexec"
    CALLER_HOME=$(getent passwd "$CALLER_UID" | cut -d: -f6)
    [[ -n "$CALLER_HOME" && -d "$CALLER_HOME" ]] || fail 10 "home directory not found"

    [[ -n "$DEST" ]] || fail 10 "missing --dest"
    DEST=$(realpath -m -- "$DEST")
    case "$DEST" in
        /var/backups|/var/backups/*|/home/*|/run/media/*|/mnt/*|/media/*) ;;
        *) fail 10 "location not allowed: $DEST" ;;
    esac

    local n rc
    for n in "${EXCLUDES[@]}"; do
        rc=0
        safe_excl_path "$n" || rc=$?
        if (( rc == 2 )); then fail 10 "this folder is required by the system and cannot be excluded: $n"; fi
        if (( rc != 0 )); then fail 10 "invalid folder to exclude: $n"; fi
    done
}

# Closest existing file above $1
existing_ancestor() {
    local p=$1
    while [[ ! -d "$p" ]]; do p=$(dirname -- "$p"); done
    printf '%s' "$p"
}

check_host_fs() {
    local info fstype opts
    info=$(findmnt -n -o FSTYPE,OPTIONS -T "$1" 2>/dev/null) \
        || fail 12 "cannot determine the filesystem of $1"
    fstype=${info%% *}
    opts=${info#* }
    case "$fstype" in
        vfat|msdos|fat)
            fail 12 "FAT32 drives cannot be used (4 GB file size limit)" ;;
        nfs|nfs4|cifs|smb3|smbfs|fuse.sshfs|9p)
            fail 12 "network locations cannot be used" ;;
        tmpfs|ramfs|devtmpfs)
            fail 12 "temporary (memory) locations cannot be used" ;;
    esac
    case ",$opts," in
        *,ro,*) fail 12 "the location is read-only" ;;
    esac
}

# Possible boot partitions; only those mounted separately (on a device other
# than /) are copied separately. If /boot is part of /,
# rsync -x already includes it.
BOOT_CANDIDATES=(/boot /boot/efi /boot/firmware)
BOOT_MOUNTS=()

boot_mounts() {
    local m root_dev dev
    root_dev=$(stat -c %d / 2>/dev/null || echo 0)
    for m in "${BOOT_CANDIDATES[@]}"; do
        mountpoint -q "$m" 2>/dev/null || continue
        dev=$(stat -c %d "$m" 2>/dev/null || echo 0)
        if [[ "$dev" != "$root_dev" ]]; then printf '%s\n' "$m"; fi
    done
}

# “Total transferred file size” from an rsync --stats output (read from standard input)
stats_size() {
    grep -m1 'Total transferred file size' | sed -E 's/[^0-9]*([0-9,]+).*/\1/; s/,//g' || true
}

# Common rsync filters for measure and backup (ONE single exclusion list)
build_rsync_args() {
    # No -h: statistics must remain in exact bytes (measure)
    RSYNC_ARGS=(-aAXHx --numeric-ids --delete)
    local e n m
    for e in /dev /proc /sys /tmp /run /mnt /media /lost+found \
             '*/thinclient_drives' '*/.gvfs' '/var/lib/dhcpcd/*'; do
        RSYNC_ARGS+=(--exclude="$e")
    done
    # Separate boot partitions: copied separately. This filter also prevents
    # the --delete option from clearing the main copy of work/<mount point>
    mapfile -t BOOT_MOUNTS < <(boot_mounts)
    for m in "${BOOT_MOUNTS[@]}"; do
        RSYNC_ARGS+=(--exclude="$m/*")
    done
    # Folder excluded by the user (absolute path): the folder is kept, but not its contents
    for n in "${EXCLUDES[@]}"; do
        RSYNC_ARGS+=(--include="$n/" --exclude="$n/**")
    done
    # Never back up images of the main disk or the destination
    # itself (image on the source disk)
    RSYNC_ARGS+=(--exclude="/var/backups/*" --exclude="$DEST/*")
}

# Options for copying a boot partition (FAT: no ACLs or extended attributes)
BOOT_ARGS=(-aHx --numeric-ids --delete --inplace --no-whole-file)

# Filters specific to a boot partition: Do not empty nested partitions
# (e.g., /boot/efi within /boot) that are copied separately
boot_nested_excludes() {   # $1 = mount point
    local m2
    for m2 in "${BOOT_MOUNTS[@]}"; do
        if [[ "$m2" == "$1"/* ]]; then printf '%s\n' "--exclude=${m2#"$1"}/*"; fi
    done
}

# Bytes to copy from the separate boot partitions to $1/<mount point>
# (BOOT_MOUNTS must be set: build_rsync_args)
boot_size() {
    local base=$1 m out size total=0 extra=()
    for m in "${BOOT_MOUNTS[@]}"; do
        mapfile -t extra < <(boot_nested_excludes "$m")
        out=$(rsync "${BOOT_ARGS[@]}" "${extra[@]}" --dry-run --stats "$m/" "$base$m/" 2>/dev/null || true)
        size=$(printf '%s\n' "$out" | stats_size)
        if is_uint "${size:-}"; then total=$(( total + size )); fi
    done
    printf '%s' "$total"
}

# ------------------------------------------------------------------ measure
cmd_measure() {
    validate_common
    build_rsync_args
    local out size
    EMPTY_DIR=$(mktemp -d)
    trap 'if [[ -n "${EMPTY_DIR:-}" ]]; then rm -rf -- "$EMPTY_DIR"; fi' EXIT
    out=$(rsync "${RSYNC_ARGS[@]}" --dry-run --stats / "$EMPTY_DIR/" 2>/dev/null || true)
    size=$(printf '%s\n' "$out" | stats_size)
    is_uint "${size:-}" || fail 41 "could not measure the size to back up"
    size=$(( size + $(boot_size "$EMPTY_DIR/data") ))
    say "RESULT:size:$size"
}

# ------------------------------------------------------------------- backup
# Clean shutdown if the app requested cancellation
check_cancel() {
    if [[ -e "$CANCEL_FILE" ]]; then
        CANCELED=1
        rm -f -- "$CANCEL_FILE"
        exit 130
    fi
}

# Copy each separate boot partition to work/<mount point>
copy_boot() {
    local m rc extra=()
    for m in "${BOOT_MOUNTS[@]}"; do
        stage boot
        mkdir -p -- "$MNT/work$m"
        mapfile -t extra < <(boot_nested_excludes "$m")
        rc=0
        rsync "${BOOT_ARGS[@]}" "${extra[@]}" "$m/" "$MNT/work$m/" 9>&- || rc=$?
        # 24 = fichiers disparus pendant la copie
        if (( rc != 0 && rc != 24 )); then
            fail 20 "copying $m failed (exit code $rc). Previous backups were not modified."
        fi
    done
}

extend_image() {   # $1 = current size, $2 = desired size (bytes, multiples of 1 MiB)
    local from=$1 to=$2 out
    if out=$(fallocate -l "$to" -- "$IMG" 2>&1); then
        return 0
    fi
    warn "fallocate-unsupported-using-dd"
    if out=$(dd if=/dev/zero of="$IMG" bs=1M seek=$(( from / 1048576 )) \
             count=$(( (to - from) / 1048576 )) conv=notrunc,fsync status=none 2>&1); then
        return 0
    fi
    LAST_ERR=$(oneline "$out")
    return 1
}

attach_loop() {
    # A run killed without cleanup (SIGKILL) can leave the image attached and mounted:
    # mounting it a second time would corrupt it
    if mountpoint -q "$MNT" 2>/dev/null || [[ -n "$(losetup -j "$IMG" -n -O NAME 2>/dev/null)" ]]; then
        fail 30 "the backup file is still in use by an earlier backup: restart the Raspberry Pi, then try again"
    fi
    LOOP_DEV=$(losetup --find --show -- "$IMG") || fail 40 "cannot attach the backup file"
}

mount_image() {
    local out kmsg
    mkdir -p -- "$MNT"
    if ! out=$(mount -t btrfs -o "$MOUNT_OPTS" -- "$LOOP_DEV" "$MNT" 2>&1); then
        kmsg=$(dmesg 2>/dev/null | grep -i btrfs | tail -n 2 || true)
        fail 40 "the backup file cannot be mounted (it was left untouched): $(oneline "$out $kmsg")"
    fi
    MOUNTED=1
}

# Bytes to be added to work/ (new or modified files), measured by a
# dry run of rsync against the image itself
measure_delta() {
    local out size
    build_rsync_args
    out=$(rsync "${RSYNC_ARGS[@]}" --dry-run --stats / "$MNT/work/" 2>/dev/null || true)
    size=$(printf '%s\n' "$out" | stats_size)
    is_uint "${size:-}" || fail 41 "could not measure what has to be copied"
    size=$(( size + $(boot_size "$MNT/work") ))
    printf '%s' "$size"
}

# Estimated free space by btrfs (bytes), including compression and profiles
btrfs_free() {
    btrfs filesystem usage -b "$MNT" 2>/dev/null | awk '/Free \(estimated\)/ {print $3; exit}'
}

# Only enlarges the image if the remaining portion to be copied does not fit: the size
# is therefore determined by ACTUAL usage (not by the uncompressed estimate)
ensure_room() {
    local delta free want grow cur avail
    delta=$(measure_delta)
    free=$(btrfs_free)
    is_uint "${free:-}" || fail 41 "cannot read the free space of the backup file"
    want=$(( delta + delta / 5 + 1073741824 ))
    warn "room: to-copy=$delta free-in-image=$free wanted=$want"
    if (( free >= want )); then return 0; fi

    grow=$(( want - free ))
    grow=$(( (grow + 1048575) / 1048576 * 1048576 ))
    cur=$(stat -c %s -- "$IMG")
    avail=$(df --output=avail -B1 -- "$DEST" | tail -n1 | tr -d ' ')
    (( avail >= grow + HOST_MARGIN )) \
        || fail 11 "not enough free space to enlarge the backup file (need $grow bytes, $avail available)"
    warn "growing the backup file by $grow bytes (from $cur)"
    extend_image "$cur" $(( cur + grow )) || fail 41 "cannot grow the backup file: $LAST_ERR"
    losetup -c "$LOOP_DEV"
    btrfs filesystem resize max "$MNT" >/dev/null
}

# Bytes actually used in the image (including btrfs, compression, and profiles)
btrfs_used() {
    local u
    u=$(btrfs filesystem usage -b "$MNT" 2>/dev/null | awk '/^[[:space:]]*Used:/ {print $2; exit}')
    is_uint "${u:-}" || return 1
    printf '%s' "$u"
}

# Target size of an image containing $1 bytes: +15% +1 Gio, minimum 4 Gio
shrink_floor() {
    local f=$(( $1 + $1 * 15 / 100 + 1073741824 ))
    if (( f < 4 * 1073741824 )); then f=$(( 4 * 1073741824 )); fi
    printf '%s' "$f"
}

# Reduces the ONLINE file system to $1 bytes (try again after compaction)
shrink_fs() {
    local out
    if out=$(btrfs filesystem resize "$1" "$MNT" 2>&1); then return 0; fi
    btrfs balance start -dusage=60 -musage=60 "$MNT" >/dev/null 2>&1 || true
    if out=$(btrfs filesystem resize "$1" "$MNT" 2>&1); then return 0; fi
    LAST_ERR=$(oneline "$out")
    return 1
}

# Verify the checksums (clear the cache to actually read the disk again)
verify_fs() {
    local out
    echo 3 > /proc/sys/vm/drop_caches 2>/dev/null || true
    if out=$(btrfs scrub start -B -- "$MNT" 2>&1); then return 0; fi
    LAST_ERR=$(oneline "$out")
    return 1
}

# Automatic reduction after a successful backup: ideally, it never causes a deadlock
auto_shrink() {
    local cur used floor
    cur=$(stat -c %s -- "$IMG")
    used=$(btrfs_used) || return 0
    floor=$(shrink_floor "$used")
    floor=$(( (floor + 1048575) / 1048576 * 1048576 ))
    warn "shrink-check: file=$cur used=$used target=$floor"
    # Only if there is at least 1 Gio AND a 15% margin to spare
    if (( cur - floor < 1073741824 || cur * 100 < floor * 115 )); then return 0; fi

    stage shrinking
    if ! shrink_fs "$floor"; then
        warn "shrink-skipped: $LAST_ERR"
        return 0
    fi
    sync
    stage verifying
    verify_fs || fail 21 "verification failed after shrinking: $LAST_ERR"
    # The file is truncated in `cleanup()` after everything has been unloaded
    PENDING_TRUNCATE=$floor
}

# An image mounted for reading should not be remounted for writing
refuse_if_browsing() {
    if mountpoint -q "$BROWSE" 2>/dev/null; then
        fail 30 "the backup is mounted for browsing at $BROWSE: unmount it first (helper umount)"
    fi
}

prune_snapshots() {
    local keep=${1:-$KEEP_SNAPSHOTS}
    local snaps=() count excess i
    mapfile -t snaps < <(find "$MNT/snapshots" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort)
    count=${#snaps[@]}
    if (( count > keep )); then
        stage pruning
        excess=$(( count - keep ))
        for (( i = 0; i < excess; i++ )); do
            btrfs subvolume delete -- "$MNT/snapshots/${snaps[i]}" >/dev/null
        done
        # Deletion is asynchronous: we wait until it has freed up the space
        btrfs subvolume sync "$MNT" >/dev/null 2>&1 || true
    fi
}

cmd_backup() {
    validate_common
    is_uint "${SIZE:-}" || fail 10 "invalid --size"
    (( SIZE >= 1073741824 && SIZE <= 17592186044416 )) || fail 10 "size out of range"
    SIZE=$(( (SIZE + 1048575) / 1048576 * 1048576 ))
    [[ -n "$CANCEL_FILE" && "$CANCEL_FILE" = /* ]] || fail 10 "invalid --cancel-file"
    resolve_keep

    trap cleanup EXIT
    # Killed (systemctl stop, kill): exit through cleanup, which unmounts and detaches
    trap 'exit 143' TERM INT HUP

    # Destination directory (e.g., <media>/astroarch/backups): created by root,
    # readable by everyone so the app can read the image size from it
    if [[ ! -d "$DEST" ]]; then
        ( umask 022; mkdir -p -- "$DEST" )
    fi
    check_host_fs "$DEST"

    mkdir -p -- "$RUN_DIR"
    exec 9>"$LOCK_FILE"
    flock -n 9 || fail 30 "another backup is already running"

    IMG="$DEST/$IMG_NAME"
    MARKER="$DEST/$MARKER_NAME"
    if [[ -L "$IMG" ]]; then fail 41 "unexpected symbolic link in place of the backup file"; fi
    refuse_if_browsing

    # --size is used only to CREATE the image; an existing image is enlarged
    # only if necessary (ensure_room), never reduced here (see `shrink`)
    local avail need=$SIZE
    if [[ -f "$IMG" ]]; then need=0; fi
    avail=$(df --output=avail -B1 -- "$DEST" | tail -n1 | tr -d ' ')
    (( avail >= need + HOST_MARGIN )) \
        || fail 11 "not enough free space at this location (need $need bytes, $avail available)"

    # ---- image preparation
    stage preparing
    unprotect_image
    if [[ ! -e "$IMG" ]]; then
        FIRST_RUN=1
        : > "$MARKER"
        if ! extend_image 0 "$SIZE"; then
            rm -f -- "$IMG" "$MARKER"
            fail 41 "cannot allocate the backup file: $LAST_ERR"
        fi
        chmod 600 -- "$IMG"
        attach_loop
        # -K : no discard (it would create holes in the preallocated file)
        local mkfs_out
        if ! mkfs_out=$(mkfs.btrfs -q -f -K -L astroarch-backup -m dup -d "$DATA_PROFILE" -- "$LOOP_DEV" 2>&1); then
            losetup -d "$LOOP_DEV" 2>/dev/null || true; LOOP_DEV=""
            rm -f -- "$IMG" "$MARKER"
            fail 41 "cannot create the btrfs filesystem: $(oneline "$mkfs_out")"
        fi
    else
        attach_loop
    fi
    mount_image

    if [[ ! -d "$MNT/work" ]]; then btrfs subvolume create "$MNT/work" >/dev/null; fi
    mkdir -p -- "$MNT/snapshots"
    ensure_room

    # ---- copie
    stage copying
    build_rsync_args
    # --inplace --no-whole-file: a modified file (log, database, etc.)
    # writes only the blocks that have changed; otherwise, Btrfs would retain, in each
    # snapshot, a COMPLETE copy of every large modified file.
    # “work/” is just a working directory: only the snapshots are authoritative
    RSYNC_ARGS+=(--no-inc-recursive --info=progress2 --inplace --no-whole-file)
    # Cancellation requested during preparation (the flag is created by the app
    # BEFORE the helper is launched, so we don't clear it here)
    check_cancel
    # 9>&- : Child processes do not keep the lock open
    stdbuf -o0 rsync "${RSYNC_ARGS[@]}" / "$MNT/work/" 9>&- &
    local rsync_pid=$!
    RSYNC_PID=$rsync_pid
    (
        exec 9>&-
        while kill -0 "$rsync_pid" 2>/dev/null; do
            if [[ -e "$CANCEL_FILE" ]]; then kill -TERM "$rsync_pid" 2>/dev/null; break; fi
            sleep 1
        done
    ) &
    WATCH_PID=$!
    local rsync_rc=0
    wait "$rsync_pid" || rsync_rc=$?
    RSYNC_PID=""
    kill "$WATCH_PID" 2>/dev/null || true
    WATCH_PID=""

    check_cancel
    # 24 = files lost during copying: normal on a running system
    if (( rsync_rc != 0 && rsync_rc != 24 )); then
        fail 20 "rsync failed (exit code $rsync_rc). Previous backups were not modified."
    fi

    # ---- Separately mounted boot partitions (e.g., /boot on FAT32)
    copy_boot
    check_cancel

    # ---- read-only snapshot
    stage snapshot
    sync
    local ts
    ts=$(date +%Y-%m-%d_%H%M%S)
    local snap_out
    if ! snap_out=$(btrfs subvolume snapshot -r -- "$MNT/work" "$MNT/snapshots/$ts" 2>&1); then
        fail 41 "snapshot creation failed: $(oneline "$snap_out")"
    fi
    rm -f -- "$MARKER"
    sync

    # ---- checksum verification (the cache is cleared to reread the disk)
    stage verifying
    verify_fs || fail 21 "verification failed, older snapshots were kept: $LAST_ERR"

    # ---- rotation: only after a successful check
    prune_snapshots "$KEEP"

    # ---- Automatic reduction if the file is significantly too large
    auto_shrink

    # ---- Final step: The app then reaches 100%
    stage finishing
    sync
    say "RESULT:ok:$ts"
}

# ------------------------------------------------------------------ - shrink
# Shrinks the image file to the actual space used (+15% +1 Gio), after
# the fact. Btrfs shrinks the file system ONLINE (it moves the data),
# then the file is truncated. Snapshots are preserved
cmd_shrink() {
    validate_common
    trap cleanup EXIT
    # Killed (systemctl stop, kill): exit through cleanup, which unmounts and detaches
    trap 'exit 143' TERM INT HUP

    IMG="$DEST/$IMG_NAME"
    MARKER="$DEST/$MARKER_NAME"
    [[ -f "$IMG" && ! -L "$IMG" ]] || fail 41 "no backup file found in $DEST"
    if [[ -e "$MARKER" ]]; then fail 41 "the first backup is unfinished: nothing to shrink yet"; fi
    check_host_fs "$DEST"

    mkdir -p -- "$RUN_DIR"
    exec 9>"$LOCK_FILE"
    flock -n 9 || fail 30 "another backup is already running"

    refuse_if_browsing
    local cur used floor target
    cur=$(stat -c %s -- "$IMG")

    stage preparing
    unprotect_image
    attach_loop
    mount_image

    used=$(btrfs_used) || fail 41 "cannot read the used space of the backup file"
    floor=$(shrink_floor "$used")

    if [[ -z "$SIZE" || "$SIZE" == auto ]]; then
        target=$floor
    else
        is_uint "$SIZE" || fail 10 "invalid --size"
        target=$SIZE
        (( target >= floor )) || fail 11 "the backup file cannot be smaller than $floor bytes (used: $used)"
    fi
    target=$(( (target + 1048575) / 1048576 * 1048576 ))

    if (( target >= cur )); then
        say "RESULT:unchanged:$cur"
        return 0
    fi

    stage shrinking
    shrink_fs "$target" || fail 41 "shrink failed (backup file left as it was): $LAST_ERR"
    sync

    stage verifying
    verify_fs || fail 21 "verification failed after shrinking: $LAST_ERR"

    # Unmount and disconnect BEFORE truncating the file
    sync
    umount -- "$MNT" || fail 41 "cannot unmount the backup file (it was NOT truncated)"
    MOUNTED=0
    losetup -d "$LOOP_DEV" || fail 41 "cannot detach the backup file (it was NOT truncated)"
    LOOP_DEV=""
    truncate -s "$target" -- "$IMG"
    say "RESULT:size:$target"
}

# ------------------------------------------------------------------- explain
# Lists what the backup would contain (same filters as “measure”), sorted by size
cmd_explain() {
    validate_common
    build_rsync_args
    EMPTY_DIR=$(mktemp -d)
    trap 'if [[ -n "${EMPTY_DIR:-}" ]]; then rm -rf -- "$EMPTY_DIR"; fi' EXIT

    local tmp
    tmp="$EMPTY_DIR/listing"
    rsync "${RSYNC_ARGS[@]}" --dry-run --out-format='%l %n' / "$EMPTY_DIR/data/" 2>/dev/null \
    | awk '
        {
            size = $1
            $1 = ""; sub(/^ /, "")
            if ($0 ~ /\/$/) next
            n = split($0, p, "/")
            k = (n >= 2) ? p[1] "/" p[2] : p[1]
            s2[k] += size
            if (p[1] == "home" && n >= 4) { h = p[1] "/" p[2] "/" p[3]; s3[h] += size }
            total += size
        }
        END {
            for (k in s2) printf "L2 %d %s\n", s2[k], k
            for (h in s3) printf "H %d %s\n", s3[h], h
            printf "T %d\n", total
        }' > "$tmp"

    say "== Largest directories that WOULD be backed up (2 levels) =="
    grep '^L2 ' "$tmp" | sort -k2 -rn | head -20 | while read -r _ sz name; do
        printf '%8s  /%s\n' "$(numfmt --to=iec "$sz")" "$name"
    done
    say ""
    say "== Inside the home directories (3 levels) =="
    grep '^H ' "$tmp" | sort -k2 -rn | head -15 | while read -r _ sz name; do
        printf '%8s  /%s\n' "$(numfmt --to=iec "$sz")" "$name"
    done
    say ""
    local total
    total=$(awk '/^T /{print $2}' "$tmp")
    say "Total: $(numfmt --to=iec "${total:-0}")"
}

# -------------------------------------------------------------- - mount/umount
# Browse the backup without damaging it: read-only loop, mount
# as “ro.” The image remains immutable (it is not unprotected)
cmd_mount() {
    validate_common
    IMG="$DEST/$IMG_NAME"
    [[ -f "$IMG" && ! -L "$IMG" ]] || fail 41 "no backup file found in $DEST"
    mkdir -p -- "$RUN_DIR"
    chmod 755 -- "$RUN_DIR"
    exec 9>"$LOCK_FILE"
    flock -n 9 || fail 30 "a backup is running"

    if mountpoint -q "$BROWSE" 2>/dev/null; then
        say "RESULT:mounted:$BROWSE"
        return 0
    fi

    local loop out
    loop=$(losetup --find --show --read-only -- "$IMG") || fail 40 "cannot attach the backup file"
    mkdir -p -- "$BROWSE"
    chmod 755 -- "$BROWSE"
    if ! out=$(mount -t btrfs -o ro,noatime -- "$loop" "$BROWSE" 2>&1); then
        losetup -d "$loop" 2>/dev/null || true
        fail 40 "cannot mount the backup file read-only: $(oneline "$out")"
    fi
    say "RESULT:mounted:$BROWSE"
}

cmd_umount() {
    validate_common
    IMG="$DEST/$IMG_NAME"
    if mountpoint -q "$BROWSE" 2>/dev/null; then
        umount -- "$BROWSE" || fail 41 "cannot unmount $BROWSE (still in use?)"
    fi
    local dev
    if [[ -e "$IMG" ]]; then
        for dev in $(losetup -j "$IMG" -O NAME -n 2>/dev/null); do
            losetup -d "$dev" 2>/dev/null || true
        done
    fi
    rmdir -- "$BROWSE" 2>/dev/null || true
    say "RESULT:unmounted"
}

# -------------------------------------------------------------------- - prune
# Deletes the oldest snapshots to keep only N (as requested),
# then reduces the file size if space allows. Deletion is irreversible
cmd_prune() {
    validate_common
    is_uint "${KEEP:-}" || fail 10 "missing or invalid --keep"
    (( KEEP >= 1 )) || fail 10 "--keep must be at least 1"
    trap cleanup EXIT
    # Killed (systemctl stop, kill): exit through cleanup, which unmounts and detaches
    trap 'exit 143' TERM INT HUP

    IMG="$DEST/$IMG_NAME"
    MARKER="$DEST/$MARKER_NAME"
    [[ -f "$IMG" && ! -L "$IMG" ]] || fail 41 "no backup file found in $DEST"
    if [[ -e "$MARKER" ]]; then fail 41 "the first backup is unfinished: nothing to prune"; fi
    check_host_fs "$DEST"

    mkdir -p -- "$RUN_DIR"
    exec 9>"$LOCK_FILE"
    flock -n 9 || fail 30 "another backup is already running"
    refuse_if_browsing

    stage preparing
    unprotect_image
    attach_loop
    mount_image

    local before after
    before=$(find "$MNT/snapshots" -mindepth 1 -maxdepth 1 -type d | wc -l)
    prune_snapshots "$KEEP"
    after=$(find "$MNT/snapshots" -mindepth 1 -maxdepth 1 -type d | wc -l)
    warn "snapshots: $before -> $after"

    auto_shrink
    say "RESULT:pruned:$(( before - after ))"
}

# -------------------------------------------------------------------- - usage
# Space used and, per snapshot, the portion that is unique to it (“Exclusive” =
# the space that would be freed if it were deleted). Mount as read-only if necessary
cmd_usage() {
    validate_common
    local was_mounted=1
    if ! mountpoint -q "$BROWSE" 2>/dev/null; then
        was_mounted=0
        cmd_mount >/dev/null
    fi
    say "== Image usage =="
    btrfs filesystem usage -h "$BROWSE" 2>&1 | sed -n '1,16p'
    say ""
    say "== Per snapshot (Exclusive = freed if that snapshot is deleted; can take a few minutes) =="
    btrfs filesystem du -s --human-readable "$BROWSE/work" "$BROWSE"/snapshots/* 2>&1
    if (( ! was_mounted )); then cmd_umount >/dev/null; fi
}

# ----------------------------------------------------------------- - snapshots
# Lists the snapshots (most recent first): SNAPSHOT:<name>:<1 if /boot is included>
cmd_snapshots() {
    validate_common
    local was_mounted=1 name boot
    if ! mountpoint -q "$BROWSE" 2>/dev/null; then
        was_mounted=0
        cmd_mount >/dev/null
    fi
    while IFS= read -r name; do
        boot=0
        if [[ -f "$BROWSE/snapshots/$name/boot/cmdline.txt" ]]; then boot=1; fi
        say "SNAPSHOT:$name:$boot"
    done < <(find "$BROWSE/snapshots" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' 2>/dev/null | sort -r)
    if (( ! was_mounted )); then cmd_umount >/dev/null; fi
    say "RESULT:snapshots"
}

# ------------------------------------------------------------------- restore
# FULL RESTORE of the system WHILE IT IS RUNNING (without another system), using a “light” takeover:
#   1. Checks without modifying anything (image, snapshot, PARTUUID/UUID, fstab, RAM);
#   2. Copy tools (bash, rsync, mount, losetup, btrfs...) ALONG WITH their libraries
#      into RAM (/run/astroarch-restore/root): they can no longer be corrupted
#      when files on the disk change;
#   3. A systemd service (IgnoreOnIsolate) shuts down the desktop and almost all services
#      (systemctl isolate), then runs rsync WITHIN this RAM-based chroot: snapshot -> root
#      (--delete-after), then snapshot/boot -> /boot;
#   4. Immediate reboot (sysrq); progress is displayed in text mode (tty1).
# Folders excluded from the backup exist in the snapshot (empty): they are therefore
# emptied. /var/backups (images) and mount points are never touched.
# A power outage during restoration may render the system unusable
readonly RESTORE_RUN="/run/astroarch-restore"
readonly RESTORE_ROOT="$RESTORE_RUN/root"
readonly RESTORE_SRC="$RUN_DIR/restore-src"
HANDED_OVER=0

restore_cleanup() {
    local rc=$?
    set +e
    if mountpoint -q "$RESTORE_SRC" 2>/dev/null; then umount -- "$RESTORE_SRC" 2>/dev/null; fi
    if [[ -n "$LOOP_DEV" ]]; then losetup -d "$LOOP_DEV" 2>/dev/null; fi
    rmdir -- "$RESTORE_SRC" 2>/dev/null
    if (( ! HANDED_OVER )); then rm -rf -- "$RESTORE_RUN"; fi
    exit "$rc"
}

# Was the snapshot created for THIS disk? (Otherwise, the Raspberry Pi wouldn't boot anymore.)
verify_identity() {
    local snap=$1 cmdline root_spec root_dev want have spec mp type opts missing=""
    cmdline=$(<"$snap/boot/cmdline.txt")
    root_spec=$(printf '%s\n' "$cmdline" | tr -s ' \t' '\n\n' | grep -m1 '^root=' | cut -d= -f2- || true)
    [[ -n "$root_spec" ]] || fail 42 "no root= found in the backup's boot/cmdline.txt"
    root_dev=$(findmnt -n -o SOURCE / | sed 's/\[.*//')

    case "$root_spec" in
        PARTUUID=*)  want=${root_spec#PARTUUID=};  have=$(blkid -c /dev/null -s PARTUUID -o value "$root_dev" 2>/dev/null || true) ;;
        UUID=*)      want=${root_spec#UUID=};      have=$(blkid -c /dev/null -s UUID -o value "$root_dev" 2>/dev/null || true) ;;
        LABEL=*)     want=${root_spec#LABEL=};     have=$(blkid -c /dev/null -s LABEL -o value "$root_dev" 2>/dev/null || true) ;;
        PARTLABEL=*) want=${root_spec#PARTLABEL=}; have=$(blkid -c /dev/null -s PARTLABEL -o value "$root_dev" 2>/dev/null || true) ;;
        /dev/*)      want=$root_spec;              have=$root_dev ;;
        *) fail 42 "unsupported root= in the backup's cmdline.txt: $root_spec" ;;
    esac
    if [[ -z "$have" || "${want,,}" != "${have,,}" ]]; then
        fail 42 "the backup was made for another disk (it starts from root=$root_spec, this system's root is ${have:-unknown}): restoring it would prevent the Raspberry Pi from starting"
    fi

    # The disks listed in the snapshot's fstab must exist (except for nofail/noauto)
    while read -r spec mp type opts _; do
        [[ -n "$spec" && "$spec" != \#* ]] || continue
        case "$spec" in
            UUID=*|PARTUUID=*|LABEL=*|PARTLABEL=*)
                if [[ -z $(blkid -c /dev/null -t "$spec" -o device 2>/dev/null || true) ]]; then
                    case ",${opts:-}," in
                        *,nofail,*|*,noauto,*) warn "fstab: $spec is not present (nofail)" ;;
                        *) missing+=" $spec" ;;
                    esac
                fi ;;
            /dev/*)
                if [[ ! -b "$spec" ]]; then
                    case ",${opts:-}," in
                        *,nofail,*|*,noauto,*) warn "fstab: $spec is not present (nofail)" ;;
                        *) missing+=" $spec" ;;
                    esac
                fi ;;
        esac
    done < "$snap/etc/fstab"
    [[ -z "$missing" ]] || fail 42 "the backup's /etc/fstab refers to disks that are not present:$missing"
}

# Restore filters (snapshot-related settings): never modified
write_restore_filters() {   # $1 = file
    local t
    {
        printf '%s\n' /dev /proc /sys /run /tmp /mnt /media /lost+found \
            '*/thinclient_drives' '*/.gvfs' '/var/lib/dhcpcd/*' '/var/backups/*' '/boot/*'
        # system mount points (e.g., /srv/astro-data mounted separately)
        while read -r t; do
            case "$t" in
                /|/boot|/boot/*|/proc|/proc/*|/sys|/sys/*|/dev|/dev/*|/run|/run/*) continue ;;
                *'*'*|*'?'*|*'['*|*']'*|*'\'*) continue ;;
            esac
            printf '%s/*\n' "$t"
        done < <(findmnt -rn -o TARGET)
    } > "$1"
}

# Copies a tool and its libraries to the in-memory environment
copy_with_libs() {   # $1 = order name
    local bin real lib
    bin=$(command -v "$1") || return 1
    real=$(readlink -f "$bin")
    install -D -m755 "$real" "$RESTORE_ROOT/usr/bin/$1"
    while read -r lib; do
        if [[ -f "$lib" ]]; then install -D -m755 "$(readlink -f "$lib")" "$RESTORE_ROOT$lib"; fi
    done < <(ldd "$real" 2>/dev/null | grep -o '/[^ ]*' | sort -u)
    return 0
}

build_ram_env() {
    local t
    rm -rf -- "$RESTORE_RUN"
    mkdir -p -- "$RESTORE_ROOT"/{usr/bin,target,src,img,proc,sys,dev,tmp,var}
    ln -sfn usr/bin "$RESTORE_ROOT/bin"
    ln -sfn usr/bin "$RESTORE_ROOT/sbin"
    ln -sfn bin "$RESTORE_ROOT/usr/sbin"
    for t in bash rsync mount umount losetup; do
        copy_with_libs "$t" || fail 41 "required tool not found: $t"
    done
    for t in btrfs sync sleep cat tee date mkdir rm cp ls sed grep stat df; do
        copy_with_libs "$t" || warn "optional tool not found: $t"
    done
    # Test before making any changes: Do the tools start up in the chroot environment?
    chroot "$RESTORE_ROOT" /usr/bin/bash -c 'true' >/dev/null 2>&1 || fail 41 "cannot prepare the restore environment (bash)"
    for t in rsync mount losetup umount; do
        chroot "$RESTORE_ROOT" "/usr/bin/$t" --version >/dev/null 2>&1 || fail 41 "cannot prepare the restore environment ($t)"
    done
}

# Script executed WITHIN the RAM-based chroot environment
write_inner_script() {
    cat > "$RESTORE_ROOT/restore.sh" <<'INNER_EOF'
#!/usr/bin/bash
# Exécuté dans le chroot en RAM. Codes de sortie : 0 ok, 1 échec AVANT toute modification,
# 2 échec PENDANT la restauration (système possiblement incohérent).
export LC_ALL=C PATH=/usr/bin
SNAPSHOT='@SNAPSHOT@'
IMGNAME='@IMGNAME@'
LOG=/restore.log
loop=""

log() { printf '%s\n' "$*" | tee -a "$LOG"; }
finish_mounts() {
    umount /src 2>/dev/null
    if [[ -n "$loop" ]]; then losetup -d "$loop" 2>/dev/null; fi
}
early() { log "ERROR: $*"; finish_mounts; exit 1; }

log "== Opening the backup =="
loop=$(losetup --find --show --read-only "/img/$IMGNAME") || early "cannot attach the backup file"
mount -t btrfs -o ro,noatime "$loop" /src || early "cannot mount the backup file"
SNAP=/src/snapshots/$SNAPSHOT
[[ -d "$SNAP" && -f "$SNAP/etc/fstab" && -f "$SNAP/boot/cmdline.txt" ]] || early "the snapshot is incomplete"
[[ -d /target/etc && -d /target/usr ]] || early "the system to restore is not reachable"
mount -o remount,rw /target/boot 2>/dev/null

log "== Restoring the system: do NOT switch off the Raspberry Pi =="
rsync -aAXH --numeric-ids --delete-after --no-inc-recursive --info=progress2 \
      --exclude-from=/filters.txt "$SNAP/" /target/
rc=$?
if (( rc != 0 && rc != 24 )); then
    log "ERROR: the system restore failed (rsync exit code $rc)"
    finish_mounts
    exit 2
fi

log "== Restoring the boot partition =="
rsync -rt --no-perms --no-owner --no-group --modify-window=2 --delete \
      "$SNAP/boot/" /target/boot/
rc=$?
if (( rc != 0 && rc != 24 )); then
    log "ERROR: the boot partition restore failed (rsync exit code $rc)"
    finish_mounts
    exit 2
fi

sync
log "== Restore complete =="
cp -f "$LOG" "/target/var/backups/astroarch-restore-$(date +%Y%m%d_%H%M%S).log" 2>/dev/null
finish_mounts
exit 0
INNER_EOF
    sed -i -e "s|@SNAPSHOT@|$SNAPSHOT|" -e "s|@IMGNAME@|$IMG_NAME|" "$RESTORE_ROOT/restore.sh"
    chmod 755 "$RESTORE_ROOT/restore.sh"
}

# Script run by the systemd service (outside of chroot)
write_run_script() {
    cat > "$RESTORE_RUN/run.sh" <<'RUN_EOF'
#!/usr/bin/bash
RAM=/run/astroarch-restore
R=$RAM/root
IMGDIR='@IMGDIR@'
say() { printf '%s\n' "$*"; }

unwind() {
    chroot "$R" /usr/bin/bash -c 'umount /target/boot; umount /target; umount /img; umount /dev; umount /sys; umount /proc' 2>/dev/null
}

# Rien n'a été modifié : on efface la préparation pour qu'une nouvelle tentative soit possible
forget() {
    cd /
    rm -rf "$RAM" /run/systemd/system/astroarch-restore.target /run/systemd/system/astroarch-restore.service
    systemctl daemon-reload
}

if command -v chvt >/dev/null 2>&1; then chvt 1 2>/dev/null; fi
say "== AstroArch restore =="
say "Stopping the desktop and the services..."
if ! systemctl isolate astroarch-restore.target; then
    say "ERROR: cannot stop the services. Nothing was changed."
    forget
    exit 1
fi
sleep 4

if ! { mount --bind / "$R/target" \
    && mount --bind /boot "$R/target/boot" \
    && mount --bind "$IMGDIR" "$R/img" \
    && mount -t proc proc "$R/proc" \
    && mount -t sysfs sysfs "$R/sys" \
    && mount --bind /dev "$R/dev"; }; then
    say "ERROR: cannot prepare the restore. Nothing was changed."
    unwind
    forget
    systemctl isolate default.target
    exit 1
fi

chroot "$R" /usr/bin/bash /restore.sh
rc=$?
unwind

case $rc in
    0)
        say "The Raspberry Pi restarts now."
        sleep 3
        echo 1 > /proc/sys/kernel/sysrq
        echo s > /proc/sysrq-trigger; sleep 2
        echo u > /proc/sysrq-trigger; sleep 2
        echo b > /proc/sysrq-trigger
        ;;
    1)
        say "The restore could not start. Nothing was changed: the desktop comes back."
        forget
        systemctl isolate default.target
        ;;
    *)
        say ""
        say "THE RESTORE DID NOT COMPLETE. The system may be unusable."
        say "Do not restart before reading the messages above."
        say "Log: $R/restore.log (until the next restart)"
        ;;
esac
RUN_EOF
    sed -i "s|@IMGDIR@|$DEST|" "$RESTORE_RUN/run.sh"
    chmod 755 "$RESTORE_RUN/run.sh"
}

# Temporary systemd units (in /run: they are deleted upon reboot)
write_restore_units() {
    local mounts=${DEST// /\\x20}
    mkdir -p /run/systemd/system
    cat > /run/systemd/system/astroarch-restore.target <<EOF
[Unit]
Description=AstroArch restore (minimal system)
Requires=sysinit.target
After=sysinit.target
AllowIsolate=yes
RequiresMountsFor=$mounts
EOF
    cat > /run/systemd/system/astroarch-restore.service <<EOF
[Unit]
Description=AstroArch system restore
DefaultDependencies=no
After=sysinit.target
IgnoreOnIsolate=yes
RequiresMountsFor=$mounts

[Service]
Type=simple
ExecStart=/usr/bin/bash $RESTORE_RUN/run.sh
StandardInput=null
StandardOutput=tty
StandardError=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
KillMode=process
EOF
}

cmd_restore() {
    validate_common
    [[ "$CONFIRM" == "RESTORE" ]] || fail 10 "confirmation missing"
    [[ "$SNAPSHOT" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}_[0-9]{6}$ ]] || fail 10 "invalid --snapshot"
    case "$DEST" in
        *\'*|*'|'*|*'&'*|*'\'*|*$'\n'*) fail 10 "unsupported character in the backup location" ;;
    esac
    [[ -d /run/systemd/system ]] && command -v systemctl >/dev/null 2>&1 || fail 43 "systemd is required"
    command -v chroot >/dev/null 2>&1 || fail 43 "chroot is required"

    IMG="$DEST/$IMG_NAME"
    MARKER="$DEST/$MARKER_NAME"
    [[ -f "$IMG" && ! -L "$IMG" ]] || fail 41 "no backup file found in $DEST"
    if [[ -e "$MARKER" ]]; then fail 41 "the first backup is unfinished: nothing to restore"; fi
    if [[ -e "$RESTORE_RUN" ]]; then fail 30 "a restore is already prepared or in progress"; fi

    mkdir -p -- "$RUN_DIR"
    exec 9>"$LOCK_FILE"
    flock -n 9 || fail 30 "a backup is running"
    refuse_if_browsing
    trap restore_cleanup EXIT

    # ---- 1. Checks, without making any changes
    stage preparing
    LOOP_DEV=$(losetup --find --show --read-only -- "$IMG") || fail 40 "cannot attach the backup file"
    mkdir -p -- "$RESTORE_SRC"
    local out
    if ! out=$(mount -t btrfs -o ro,noatime -- "$LOOP_DEV" "$RESTORE_SRC" 2>&1); then
        fail 40 "the backup file cannot be mounted: $(oneline "$out")"
    fi
    local snap="$RESTORE_SRC/snapshots/$SNAPSHOT"
    [[ -d "$snap" ]] || fail 41 "snapshot not found: $SNAPSHOT"
    if [[ ! -f "$snap/boot/cmdline.txt" || ! -f "$snap/etc/fstab" ]]; then
        fail 41 "this backup does not contain the boot partition: it cannot be restored completely (make a new backup)"
    fi
    stage verifying
    verify_identity "$snap"

    local avail
    avail=$(df --output=avail -B1 -- /run | tail -n1 | tr -d ' ')
    (( avail >= 200 * 1048576 )) || fail 41 "not enough free memory to prepare the restore"

    # ---- 2. RAM environment + restore files
    stage handover
    build_ram_env
    write_restore_filters "$RESTORE_ROOT/filters.txt"
    write_inner_script
    write_run_script
    write_restore_units

    # The validation opened the image: we're releasing it, and the service will reopen it on its own
    umount -- "$RESTORE_SRC" || fail 41 "cannot release the backup file"
    losetup -d "$LOOP_DEV" && LOOP_DEV=""

    # ---- 3. Start: This command ends, and the service takes over
    systemctl daemon-reload
    HANDED_OVER=1
    systemctl start --no-block astroarch-restore.service
    say "RESULT:restore-started"
}

# ------------------------------------------------------------------ datadir
# The "Data and settings" backup (astroarch-backup --data-backup) runs as the user, every
# 2 hours, without root. It only needs its folder, created here once and owned by the
# caller: /var/backups/astroarch-data/<user> on the main disk (never in the system backup),
# or <mounted drive>/astroarch/data-backup. Nothing else is changed
cmd_datadir() {
    validate_common
    local user group base
    user=$(id -un -- "$CALLER_UID") || fail 10 "unknown user"
    group=$(id -g -- "$CALLER_UID") || fail 10 "unknown user"
    case "$DEST" in
        "/var/backups/astroarch-data/$user") base=/var/backups ;;
        /run/media/*/astroarch/data-backup|/media/*/astroarch/data-backup|/mnt/*/astroarch/data-backup)
            base=${DEST%/astroarch/data-backup}
            # A drive that is really mounted there, not a folder of the SD card
            mountpoint -q -- "$base" || fail 12 "the drive is not mounted: $base"
            [[ $(stat -c %d -- "$base") != $(stat -c %d /) ]] || fail 12 "this is the system disk: $base" ;;
        *) fail 10 "location not allowed: $DEST" ;;
    esac
    check_host_fs "$(existing_ancestor "$DEST")"
    # Parent folders readable by everyone (the helper's umask is 077)
    umask 022
    local parent=${DEST%/*} name=${DEST##*/}
    mkdir -p -- "$parent"
    # Created from inside the real parent folder: a symbolic link swapped in meanwhile
    # cannot send root elsewhere
    cd -P -- "$parent" || fail 12 "cannot open $parent"
    [[ $(pwd -P) == "$parent" ]] || fail 12 "symbolic links are not allowed: $DEST"
    if [[ -e "$name" || -L "$name" ]]; then
        [[ ! -L "$name" && -d "$name" ]] || fail 12 "not a folder: $DEST"
        [[ $(stat -c %u -- "$name") == "$CALLER_UID" ]] || fail 12 "this folder belongs to another user: $DEST"
    else
        mkdir -m 750 -- "$name" || fail 12 "cannot create $DEST"
        chown -h -- "$CALLER_UID:$group" "$name"
    fi
    say "RESULT:ok:$DEST"
}

# ---------------------------------------------------------------- nas-add/remove
# A network share (NAS) for the "Data and settings" backup, kept across restarts: an
# /etc/fstab entry mounted on first use (x-systemd.automount) that never blocks the boot
# (nofail, _netdev), on /mnt/astroarch-nas/<id>, owned by the caller. SMB credentials in
# /etc/astroarch-backup/nas-<id>.cred (root only, 0600); the password comes on stdin,
# never on a command line. The share is mounted once to check it before anything is kept
readonly NAS_BASE=/mnt/astroarch-nas
readonly NAS_CRED_DIR=/etc/astroarch-backup
readonly NAS_MARK="# astroarch-backup nas"

nas_caller() {
    (( EUID == 0 )) || fail 10 "must run as root"
    CALLER_UID=${PKEXEC_UID:-}
    is_uint "$CALLER_UID" || fail 10 "must be started through pkexec"
    CALLER_GID=$(id -g -- "$CALLER_UID") || fail 10 "unknown user"
}

cmd_nas_add() {
    nas_caller
    # Same rules as the Control Center (src/data_backup.rs): checked here again
    [[ $NAS_SERVER =~ ^[A-Za-z0-9][A-Za-z0-9.-]{0,252}$ || $NAS_SERVER =~ ^\[?[0-9A-Fa-f:]{2,39}\]?$ ]] \
        || fail 10 "invalid server name"
    local fstype
    case $NAS_KIND in
        smb)
            [[ $NAS_SHARE =~ ^[A-Za-z0-9._\ \$-]{1,80}$ && $NAS_SHARE != *..* ]] || fail 10 "invalid share name"
            [[ -z $NAS_USER || $NAS_USER =~ ^[A-Za-z0-9._@-]{1,64}$ ]] || fail 10 "invalid user name"
            command -v mount.cifs >/dev/null || fail 12 "cifs-utils is not installed"
            fstype=cifs ;;
        nfs)
            [[ $NAS_SHARE =~ ^/[A-Za-z0-9._/-]{0,200}$ && $NAS_SHARE != *..* ]] || fail 10 "invalid export path"
            command -v mount.nfs >/dev/null || fail 12 "nfs-utils is not installed"
            fstype=nfs ;;
        *) fail 10 "invalid --kind" ;;
    esac
    local id mnt cred="" src opts pass="" out
    id=$(printf '%s-%s' "$NAS_SERVER" "$NAS_SHARE" | tr 'A-Z' 'a-z' | tr -c 'a-z0-9' '-' | tr -s '-' | sed 's/^-//; s/-$//' | cut -c1-60)
    [[ -n $id ]] || fail 10 "invalid share"
    mnt=$NAS_BASE/$id
    grep -qxF "$NAS_MARK $id" /etc/fstab && fail 12 "this share is already connected"
    findmnt -rn --fstab -T "$mnt" >/dev/null 2>&1 && fail 12 "$mnt is already in /etc/fstab"

    if [[ $NAS_KIND == smb ]]; then
        # One line on stdin
        IFS= read -r pass || true
        [[ $pass != *$'\r'* ]] || fail 10 "invalid password"
        src="//$NAS_SERVER/$NAS_SHARE"
        opts="uid=$CALLER_UID,gid=$CALLER_GID,file_mode=0640,dir_mode=0750,iocharset=utf8"
        if [[ -n $NAS_USER ]]; then
            ( umask 077; mkdir -p -- "$NAS_CRED_DIR" )
            chmod 700 -- "$NAS_CRED_DIR"
            cred=$NAS_CRED_DIR/nas-$id.cred
            ( umask 077; printf 'username=%s\npassword=%s\n' "$NAS_USER" "$pass" > "$cred" )
            opts="credentials=$cred,$opts"
        else
            opts="guest,$opts"
        fi
    else
        src="$NAS_SERVER:$NAS_SHARE"
        opts="soft,noatime"
    fi
    ( umask 022; mkdir -p -- "$mnt" )
    # Mounted once, to check the server, the share and the password
    if ! out=$(timeout 40 mount -t "$fstype" -o "$opts" -- "$src" "$mnt" 2>&1); then
        [[ -n $cred ]] && rm -f -- "$cred"
        rmdir -- "$mnt" "$NAS_BASE" "$NAS_CRED_DIR" 2>/dev/null || true
        fail 12 "cannot connect to $src: $(oneline "$out")"
    fi
    umount -- "$mnt" || umount -l -- "$mnt"
    # A space in fstab is written \040
    printf '%s %s\n%s %s %s %s,nofail,_netdev,x-systemd.automount,x-systemd.mount-timeout=20,x-systemd.idle-timeout=600 0 0\n' \
        "$NAS_MARK" "$id" "${src// /\\040}" "$mnt" "$fstype" "$opts" >> /etc/fstab
    systemctl daemon-reload
    systemctl start "$(systemd-escape -p --suffix=automount -- "$mnt")" || warn "automount not started"
    say "RESULT:ok:$mnt"
}

cmd_nas_remove() {
    nas_caller
    [[ $NAS_ID =~ ^[a-z0-9-]{1,60}$ ]] || fail 10 "invalid --id"
    grep -qxF "$NAS_MARK $NAS_ID" /etc/fstab || fail 12 "no such share"
    local mnt=$NAS_BASE/$NAS_ID unit tmp
    unit=$(systemd-escape -p -- "$mnt")
    systemctl stop "$unit.automount" "$unit.mount" 2>/dev/null || true
    umount -l -- "$mnt" 2>/dev/null || true
    # The mark and the line after it, nothing else; the file keeps its owner and mode
    tmp=$(mktemp /etc/fstab.astroarch-XXXXXX)
    awk -v mark="$NAS_MARK $NAS_ID" '$0 == mark { skip = 1; next } skip { skip = 0; next } { print }' /etc/fstab > "$tmp" \
        && cat -- "$tmp" > /etc/fstab
    rm -f -- "$tmp" "$NAS_CRED_DIR/nas-$NAS_ID.cred"
    rmdir -- "$mnt" "$NAS_BASE" "$NAS_CRED_DIR" 2>/dev/null || true
    systemctl daemon-reload
    say "RESULT:ok:$NAS_ID"
}

main() {
    local cmd=${1:-}
    shift || true
    parse_args "$@"
    case "$cmd" in
        measure) cmd_measure ;;
        backup)  cmd_backup ;;
        shrink)  cmd_shrink ;;
        explain) cmd_explain ;;
        mount)   cmd_mount ;;
        umount)  cmd_umount ;;
        usage)   cmd_usage ;;
        prune)   cmd_prune ;;
        snapshots) cmd_snapshots ;;
        restore) cmd_restore ;;
        datadir) cmd_datadir ;;
        nas-add) cmd_nas_add ;;
        nas-remove) cmd_nas_remove ;;
        *)       fail 10 "unknown command: $cmd" ;;
    esac
}

main "$@"

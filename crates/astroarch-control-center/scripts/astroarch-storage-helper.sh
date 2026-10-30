#!/bin/bash
# astroarch-storage-helper move DEVICE | back | reconnect | forget
#
# Moves the shared data folder /srv/astro-data (astronaut and astronaut-kiosk, group
# astro) to a drive, and back. Run as root with pkexec by the Image storage page of the
# Control Center (org.astroarch.control-center.storage.policy); the page itself never
# changes the system. Installed as /usr/lib/astroarch-control-center/astroarch-storage-helper.
# It never formats a drive.
#
# On a drive the data is in <drive>/astroarch/astro-data, mounted at /srv/astro-data by
# one /etc/fstab line (by UUID; nofail: AstroArch starts without the drive). A udev rule
# mounts it again when the drive is plugged back in. Without the drive, /srv/astro-data
# is an empty folder nobody can write in (chattr +i): programs get an error instead of
# filling the internal disk under the mount point.
#
#   move DEVICE   copy the folder to the partition DEVICE, then use it from there; the
#                 internal copy is deleted once everything is copied
#   back          copy the folder back to the internal disk, stop using the drive (the
#                 files stay on the drive too)
#   reconnect     mount the drive again (it was unplugged, or unmounted by hand)
#   forget        stop using a drive that is not connected (its files stay on it)
#
# Output on stdout, one record per line: STAGE:<prepare|copy|switch|done>,
# PROGRESS:<percent>, ERROR:<code>:<message>. Exit code 0 = done.
#
# ASTRO_DATA_DIR replaces /srv/astro-data, for tests only (pkexec clears the environment).

set -u
export LC_ALL=C
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin

DATA=${ASTRO_DATA_DIR:-/srv/astro-data}
DATA=${DATA%/}
GROUP=astro
SUBDIR=astroarch/astro-data
FSTAB=/etc/fstab
# The line above the fstab entry; it also keeps the drive's name for when it is unplugged
MARK="# astroarch-storage:"
UNIT=$(systemd-escape -p --suffix=mount "$DATA")
RULES=/etc/udev/rules.d/90-astroarch-storage${ASTRO_DATA_DIR:+-test}.rules
NOTICE=DRIVE-NOT-CONNECTED.txt
LOCK=/run/astroarch-storage.lock
# Default ACL of /srv/astro-data, as in configs/astro-data.conf of the astroarch repository
ACL=default:user::rwx,default:group::rwx,default:mask::rwx,default:other::r-x

say()  { printf '%s\n' "$*"; }
fail() { local code=$1; shift; say "ERROR:$code:$*"; exit 1; }

# File systems with owners and permissions, and those that get them from mount options
POSIX_FS=" ext4 ext3 ext2 btrfs xfs f2fs "
SHARED_FS=" exfat vfat ntfs "
is_posix() { [[ $POSIX_FS == *" $1 "* ]]; }

# -------------------------------------------------------------- reading the system

# Disk names (sda, mmcblk0...) holding /, /boot or /boot/firmware
system_disks() {
    local m src
    for m in / /boot /boot/firmware; do
        src=$(findmnt -nro SOURCE "$m" 2>/dev/null) || continue
        src=${src%%[*}
        [[ -b $src ]] && disk_of "$src"
    done
}

# Disk of a partition (the device itself for a file system without partitions)
disk_of() {
    local parent
    parent=$(lsblk -dno PKNAME "$1" 2>/dev/null | head -n1)
    [[ -n $parent ]] && echo "$parent" || lsblk -dno NAME "$1"
}

# The fstab entry of the folder (fields: UUID=... DATA type options), if any
fstab_entry() {
    awk -v d="$DATA" '$1 !~ /^#/ && $2 == d { print; exit }' "$FSTAB"
}

# Programs using the folder (current folder or open file), and KStars, which may save in
# it at any moment
users_of_data() {
    local dir pids
    dir=$(readlink -f "$DATA")
    pids=$(find /proc/[0-9]*/cwd /proc/[0-9]*/fd -maxdepth 1 \( -lname "$dir" -o -lname "$dir/*" \) 2>/dev/null \
           | cut -d/ -f3 | sort -u)
    pids="$pids $(pgrep -x kstars)"
    for p in $pids; do
        [[ $p == "$$" ]] && continue
        cat "/proc/$p/comm" 2>/dev/null
    done | sort -u | paste -sd, -
}

# Kilobytes used by a folder, and free in the file system holding a path
used_kib() { du -sxk "$1" 2>/dev/null | cut -f1; }
free_kib() { df -Pk "$1" 2>/dev/null | awk 'NR == 2 { print $4 }'; }

# Mount options for a drive of this type: owners and permissions from the folder itself
# on ext4/btrfs..., else group astro and group-writable for everything
mount_type() { [[ $1 == ntfs ]] && echo ntfs3 || echo "$1"; }
mount_options() {
    local gid
    gid=$(getent group "$GROUP" | cut -d: -f3)
    case $1 in
        exfat) echo "uid=0,gid=$gid,dmask=0002,fmask=0113,allow_utime=0020" ;;
        vfat)  echo "uid=0,gid=$gid,dmask=0002,fmask=0113,allow_utime=0020,iocharset=utf8,shortname=mixed" ;;
        ntfs)  echo "uid=0,gid=$gid,dmask=0002,fmask=0113" ;;
        *)     echo "noatime" ;;
    esac
}

# --------------------------------------------------------------- changing it

# The folder's own permissions on the internal disk (as systemd-tmpfiles sets them)
set_folder_permissions() {
    chown root:"$GROUP" "$1" && chmod 2775 "$1" && { setfacl -m "$ACL" "$1" 2>/dev/null || true; }
}

# Copy the content of $1 into $2 with the progress on stdout; $3 = posix|shared (the
# kind of the file system written to or read from). 24 = some files vanished meanwhile
copy_folder() {
    local opts err rc
    if [[ $3 == posix ]]; then opts=(-aHAX); else opts=(-rt --modify-window=1); fi
    err=$(mktemp)
    # Should the page go away, cat drains the rest of the progress: the copy goes on
    rsync "${opts[@]}" --no-inc-recursive --info=progress2 "$1/" "$2/" 2>"$err" \
        | { awk -v RS='[\r\n]+' 'match($0, /([0-9]+)%/, m) && m[1] != last { last = m[1]; print "PROGRESS:" m[1]; fflush() }' 2>/dev/null
            cat > /dev/null; }
    rc=${PIPESTATUS[0]}
    if [[ $rc != 0 && $rc != 24 ]]; then
        say "ERROR:copy:$(tail -n 3 "$err" | tr '\n' ' ')"
        rm -f "$err"
        return 1
    fi
    rm -f "$err"
}

# Mount the whole partition on a temporary folder (path on stdout)
mount_drive() {
    local dir
    dir=$(mktemp -d /run/astroarch-storage.XXXXXX) || return 1
    if mount -t "$(mount_type "$2")" -o "$(mount_options "$2")" "$1" "$dir"; then
        echo "$dir"
    else
        rmdir "$dir"
        return 1
    fi
}
unmount_drive() { [[ -n ${TMP_MOUNT:-} ]] && umount "$TMP_MOUNT" 2>/dev/null && rmdir "$TMP_MOUNT"; TMP_MOUNT=; }
trap 'unmount_drive' EXIT

# The fstab entry and the udev rule that mounts the drive again when it is plugged in
write_config() {   # uuid type label
    local tmp
    cp -p "$FSTAB" "$FSTAB.astroarch-storage.bak" || return 1
    tmp=$(mktemp "$FSTAB.XXXXXX") || return 1
    { cat "$FSTAB"
      [[ -z $(tail -c1 "$FSTAB") ]] || echo
      printf '%s %s\n' "$MARK" "$3"
      printf 'UUID=%s %s %s %s,nofail,x-systemd.device-timeout=10s,X-mount.subdir=%s 0 0\n' \
             "$1" "$DATA" "$(mount_type "$2")" "$(mount_options "$2")" "$SUBDIR"
    } > "$tmp" && chmod 644 "$tmp" && mv "$tmp" "$FSTAB" || { rm -f "$tmp"; return 1; }
    printf '# Image storage of the Control Center: mount %s again when the drive is plugged in\n' "$DATA" > "$RULES"
    printf 'SUBSYSTEM=="block", ENV{ID_FS_UUID}=="%s", ENV{SYSTEMD_WANTS}+="%s"\n' "$1" "$UNIT" >> "$RULES"
    systemctl daemon-reload
    udevadm control --reload
}
remove_config() {
    local tmp
    tmp=$(mktemp "$FSTAB.XXXXXX") || return 1
    awk -v d="$DATA" -v mark="$MARK" '
        index($0, mark) == 1 { next }
        $1 !~ /^#/ && $2 == d { next }
        { print }' "$FSTAB" > "$tmp" && chmod 644 "$tmp" && mv "$tmp" "$FSTAB" || { rm -f "$tmp"; return 1; }
    rm -f "$RULES"
    systemctl daemon-reload
    udevadm control --reload
}

# The internal folder while the data is on the drive: empty but for a note, and
# unwritable even by root, so nothing lands on the internal disk without the drive
protect_internal() {   # label
    find "$DATA" -mindepth 1 -maxdepth 1 ! -name .directory -exec rm -rf {} + || return 1
    printf 'Your AstroArch images are on the drive "%s", which is not connected.\nPlug it in: this folder shows them again. Nothing can be saved here meanwhile.\n' \
        "$1" > "$DATA/$NOTICE"
    set_folder_permissions "$DATA"
    chattr +i "$DATA"
}
unprotect_internal() {
    chattr -i "$DATA" 2>/dev/null
    rm -f "$DATA/$NOTICE"
    set_folder_permissions "$DATA"
}

# ------------------------------------------------------------------ commands

cmd_move() {
    local dev=${1:-} type uuid label ro free need users m
    [[ $dev =~ ^/dev/[A-Za-z0-9/_.-]+$ && -b $dev ]] || fail device "Not a drive: $dev"
    [[ -d $DATA ]] || fail folder "$DATA does not exist"
    [[ -z $(fstab_entry) ]] && ! mountpoint -q "$DATA" || fail already "$DATA is already on a drive"

    type=$(lsblk -dno FSTYPE "$dev"); uuid=$(lsblk -dno UUID "$dev")
    label=$(lsblk -dno LABEL "$dev"); ro=$(lsblk -dno RO "$dev")
    [[ -n $label ]] || label=$(lsblk -dno MODEL "/dev/$(disk_of "$dev")" | sed 's/ *$//')
    [[ -n $label ]] || label=${dev#/dev/}
    label=${label//[$'\n\r']/ }
    system_disks | grep -qxF "$(disk_of "$dev")" && fail system "$dev is on the system disk"
    findmnt -nro TARGET -S "$dev" | grep -qE '^/(boot.*)?$' && fail system "$dev is the system"
    [[ $label == ALARM_* ]] && fail system "$dev holds a system ($label)"
    [[ $ro == *1* ]] && fail readonly "$dev is read-only"
    [[ $POSIX_FS$SHARED_FS == *" $type "* ]] || fail unsupported "${type:-no file system}"
    [[ -n $uuid ]] || fail unsupported "$dev has no UUID"
    users=$(users_of_data); [[ -z $users ]] || fail busy "$users"

    say "STAGE:prepare"
    # Mounted by the file manager (udisks): unmounted, then mounted with our options
    while read -r m; do
        [[ -n $m ]] || continue
        umount "$m" || fail drive-busy "$dev is in use at $m"
    done < <(findmnt -nro TARGET -S "$dev")
    TMP_MOUNT=$(mount_drive "$dev" "$type") || fail mount "Cannot mount $dev ($type)"
    mkdir -p "$TMP_MOUNT/$SUBDIR" || fail mount "Cannot write on $dev"
    is_posix "$type" && set_folder_permissions "$TMP_MOUNT/$SUBDIR"
    need=$(( $(used_kib "$DATA") * 105 / 100 + 65536 ))
    free=$(free_kib "$TMP_MOUNT")
    (( free >= need )) || fail space "$need $free"

    say "STAGE:copy"
    copy_folder "$DATA" "$TMP_MOUNT/$SUBDIR" "$(is_posix "$type" && echo posix || echo shared)" || exit 1
    sync

    say "STAGE:switch"
    unmount_drive
    write_config "$uuid" "$type" "$label" || fail config "Cannot write $FSTAB"
    protect_internal "$label" || fail cleanup "The files were copied, but the internal folder could not be emptied"
    systemctl start "$UNIT" && mountpoint -q "$DATA" || fail mount "Copied, but the drive could not be mounted at $DATA"
    say "STAGE:done"
}

cmd_back() {
    local entry uuid type dev need free users
    entry=$(fstab_entry); [[ -n $entry ]] || fail not-moved "$DATA is on the internal disk"
    read -r uuid _ type _ <<< "$entry"; uuid=${uuid#UUID=}
    [[ $type == ntfs3 ]] && type=ntfs
    dev=$(blkid -U "$uuid") || fail drive-missing "The drive $uuid is not connected"
    users=$(users_of_data); [[ -z $users ]] || fail busy "$users"

    say "STAGE:prepare"
    TMP_MOUNT=$(mount_drive "$dev" "$type") || fail mount "Cannot mount $dev ($type)"
    [[ -d $TMP_MOUNT/$SUBDIR ]] || fail mount "No $SUBDIR on the drive"
    need=$(( $(used_kib "$TMP_MOUNT/$SUBDIR") + 65536 ))
    free=$(free_kib "$(dirname "$DATA")")
    (( free >= need )) || fail space "$need $free"

    say "STAGE:switch"
    if mountpoint -q "$DATA"; then
        systemctl stop "$UNIT"; mountpoint -q "$DATA" && umount "$DATA"
        mountpoint -q "$DATA" && fail busy "$(users_of_data)"
    fi
    remove_config || fail config "Cannot write $FSTAB"
    unprotect_internal

    say "STAGE:copy"
    if ! copy_folder "$TMP_MOUNT/$SUBDIR" "$DATA" "$(is_posix "$type" && echo posix || echo shared)"; then
        # As before: on the drive, the partial copy removed
        write_config "$uuid" "$type" "$LABEL_KEPT" && protect_internal "$LABEL_KEPT" && systemctl start "$UNIT"
        exit 1
    fi
    # From exFAT/FAT/NTFS the files come without owners: group astro, group-writable
    if ! is_posix "$type"; then
        chgrp -R "$GROUP" "$DATA" && chmod -R g+rwX "$DATA" && find "$DATA" -type d -exec chmod g+s {} +
    fi
    set_folder_permissions "$DATA"
    say "STAGE:done"
}

cmd_reconnect() {
    local entry uuid
    entry=$(fstab_entry); [[ -n $entry ]] || fail not-moved "$DATA is on the internal disk"
    uuid=${entry%% *}; uuid=${uuid#UUID=}
    blkid -U "$uuid" > /dev/null || fail drive-missing "The drive $uuid is not connected"
    say "STAGE:switch"
    systemctl start "$UNIT" && mountpoint -q "$DATA" || fail mount "The drive could not be mounted at $DATA"
    say "STAGE:done"
}

cmd_forget() {
    [[ -n $(fstab_entry) ]] || fail not-moved "$DATA is on the internal disk"
    mountpoint -q "$DATA" && fail connected "The drive is connected: move the folder back instead"
    say "STAGE:switch"
    remove_config || fail config "Cannot write $FSTAB"
    unprotect_internal
    say "STAGE:done"
}

# Closing the Control Center window does not stop the helper half-way
trap '' PIPE HUP

[[ $EUID == 0 ]] || fail root "Must run as root"
exec 9> "$LOCK"
flock -n 9 || fail running "Already running"
# The drive's name kept above the fstab entry
LABEL_KEPT=$(sed -n "s/^$MARK //p" "$FSTAB" | head -n1)

case ${1:-} in
    move)      cmd_move "${2:-}" ;;
    back)      cmd_back ;;
    reconnect) cmd_reconnect ;;
    forget)    cmd_forget ;;
    *)         fail usage "astroarch-storage-helper move DEVICE | back | reconnect | forget" ;;
esac

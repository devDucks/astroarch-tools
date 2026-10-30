#!/bin/bash
# astroarch-restore-helper mount DEVICE | unmount | restore | undo FOLDER...
#
# The "Restore from a backup" page of the Control Center, the part run as root with
# pkexec (org.astroarch.control-center.restore.policy). Installed as
# /usr/lib/astroarch-control-center/astroarch-restore-helper.
#
#   mount DEVICE  mount the partition of an old AstroArch disk READ-ONLY (the device is
#                 set read-only too when nobody uses it) under /run/astroarch-cc-restore
#   unmount       unmount what `mount` mounted
#   restore       copy what the page chose, read from stdin, one line each (tab separated):
#                   label   TEXT                      what the restore came from
#                   root    PATH                      every source path is inside one: a
#                                                     folder of a backup, an old disk mounted
#                                                     by `mount`, or a snapshot of the system
#                                                     backup mounted read-only by the backup
#                                                     helper (/run/astroarch-backup/browse/
#                                                     snapshots/NAME); sources only in its
#                                                     home/ and srv/astro-data
#                   exclude PATH                      never copied
#                   settings|data USER SOURCE TARGET
#                 The files are written by USER (the main user or its kiosk user), so they
#                 belong to them and land only where they can write. They are READ by root
#                 only from the read-only mounts made by root itself (an old disk this helper
#                 set read-only, a snapshot of the backup's browse mount, held open during
#                 the restore so it cannot be swapped); from any other folder (a data backup,
#                 the AstroMonitor download, an old disk a file manager had mounted) they are
#                 read as the main user: its own files, nothing more. Each source is resolved
#                 once, when checked, and only that path is used; links inside are copied
#                 as links, never followed. settings: a file already there is moved aside first, to
#                 ~/.local/share/astroarch/restore-DATE/files (with the list of what was
#                 restored, for `undo`); data (images): files already there are kept
#   undo FOLDER   put back what a restore moved aside (FOLDER: a restore-DATE folder)
#
# Refuses while KStars, PHD2 or an INDI server run (any user): they would write over the
# restored files. Output on stdout, one record per line: STAGE:<copy|done>,
# PROGRESS:<entry>:<entries>, ASIDE:<folder>, SKIPPED:<reason>:<path>, BUSY:<program>,
# RESULT:<value>, ERROR:<code>:<message>. Exit code 0 = done, 4 = busy.
#
# For tests only (pkexec clears the environment): ASTROARCH_MAIN_USER, ASTROARCH_KIOSK_USER,
# ASTRO_DATA_DIR, ASTROARCH_KIOSK_SETUP replace the users, /srv/astro-data and the kiosk helper.

set -u
export LC_ALL=C
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin

MOUNTS=/run/astroarch-cc-restore
# Where the backup helper (crates/astroarch-backup) mounts the system backup read-only
BROWSE=/run/astroarch-backup/browse
DATA=${ASTRO_DATA_DIR:-/srv/astro-data}
DATA=${DATA%/}
KIOSK_SETUP=${ASTROARCH_KIOSK_SETUP:-/usr/lib/astroarch/astroarch-kiosk-setup}
BUSY_APPS=(kstars phd2.bin indiserver)

say()  { printf '%s\n' "$*"; }
fail() { local code=$1; shift; say "ERROR:$code:$*"; exit 1; }

# Run from / : the users may not enter the caller's folder
cd / || exit 1

# The user who asked (pkexec), and its kiosk user if there is one
MAIN_USER=${ASTROARCH_MAIN_USER:-${PKEXEC_UID:+$(id -nu "$PKEXEC_UID" 2>/dev/null)}}
KIOSK_USER=${ASTROARCH_KIOSK_USER:-${MAIN_USER:+$MAIN_USER-kiosk}}
home_of() { getent passwd "$1" | cut -d: -f6; }

# Is $1 (resolved) inside $2 (resolved)?
inside() { [[ $1 == "$2" || $1 == "$2"/* ]]; }

# Disk names (sda, mmcblk0...) holding /, /boot or /boot/firmware
system_disks() {
    local m src
    for m in / /boot /boot/firmware; do
        src=$(findmnt -nro SOURCE "$m" 2>/dev/null) || continue
        src=${src%%[*}
        [[ -b $src ]] && disk_of "$src"
    done
}
disk_of() {
    local parent
    parent=$(lsblk -dno PKNAME "$1" 2>/dev/null | head -n1)
    [[ -n $parent ]] && echo "$parent" || lsblk -dno NAME "$1"
}

check_not_busy() {
    local p
    for p in "${BUSY_APPS[@]}"; do
        if pgrep -x "$p" > /dev/null; then
            say "BUSY:$p"
            exit 4
        fi
    done
}

# -------------------------------------------------------------------- old disk

cmd_mount() {
    local dev=${1:-} name type opts mp
    [[ $dev =~ ^/dev/[A-Za-z0-9/_.-]+$ && -b $dev ]] || fail device "Not a drive: $dev"
    grep -qxF "$(disk_of "$dev")" <<< "$(system_disks)" && fail system "$dev is on the system disk"
    name=${dev##*/}
    mp=$MOUNTS/$name
    if mountpoint -q "$mp"; then
        say "RESULT:$mp"
        return
    fi
    type=$(lsblk -dno FSTYPE "$dev")
    # Never a write, not even the replay of a journal
    case $type in
        ext2 | ext3 | ext4) opts=ro,noload ;;
        btrfs) opts=ro,rescue=nologreplay ;;
        xfs) opts=ro,norecovery ;;
        f2fs) opts=ro,norecovery ;;
        *) fail unsupported "${type:-no file system}" ;;
    esac
    mkdir -p "$mp" || fail mount "Cannot create $mp"
    # The device itself read-only, unless a file manager has it mounted already
    if [[ -z $(findmnt -nro TARGET -S "$dev") ]]; then
        blockdev --setro "$dev" && : > "$MOUNTS/.setro-$name"
    fi
    if ! mount -t "$type" -o "$opts" "$dev" "$mp" 2> /dev/null && ! mount -o ro "$dev" "$mp"; then
        rmdir "$mp"
        [[ -e $MOUNTS/.setro-$name ]] && blockdev --setrw "$dev" && rm -f "$MOUNTS/.setro-$name"
        fail mount "Cannot mount $dev ($type)"
    fi
    say "RESULT:$mp"
}

cmd_unmount() {
    local mp name
    for mp in "$MOUNTS"/*/; do
        mp=${mp%/}
        [[ -d $mp ]] || continue
        name=${mp##*/}
        mountpoint -q "$mp" && { umount "$mp" || fail busy "$mp is in use"; }
        rmdir "$mp"
        if [[ -e $MOUNTS/.setro-$name ]]; then
            blockdev --setrw "/dev/$name" 2> /dev/null
            rm -f "$MOUNTS/.setro-$name"
        fi
    done
    say "RESULT:ok"
}

# --------------------------------------------------------------------- restore

ROOTS=()
EXCLUDES=()
declare -A TRUSTED      # resolved root -> 1: a read-only mount made by root, read as root
declare -A ASIDE        # user -> restore-DATE folder, made at its first file
STAMP=$(date +%Y%m%d-%H%M%S)
LABEL=""

# The restore-DATE folder of a user (ASIDE[user]), created on first use
make_aside() {
    local user=$1 dir
    [[ -n ${ASIDE[$user]:-} ]] && return 0
    dir=$(home_of "$user")/.local/share/astroarch/restore-$STAMP
    runuser -u "$user" -- mkdir -p "$dir/files" || return 1
    printf '%s\n%s\n' "$LABEL" "$(date +%s)" | runuser -u "$user" -- tee "$dir/info" > /dev/null
    ASIDE[$user]=$dir
}

# Where a user may get files: its home, and the main user also the image folder
target_ok() {   # user target
    local base dir
    base=$(realpath -e "$(home_of "$1")" 2>/dev/null) || return 1
    # The folder that will hold it, resolved: a link that leads out is refused
    dir=$(realpath -m "$(dirname "$2")") || return 1
    [[ $2 != *"/../"* && $2 != *"/./"* ]] || return 1
    inside "$dir/${2##*/}" "$base" && return 0
    [[ $1 == "$MAIN_USER" ]] && inside "$dir/${2##*/}" "$(realpath -m "$DATA")"
}

# A root to read from (resolved): never / itself or a kernel folder; under the folders the
# helpers mount, only an old disk mounted here or a snapshot of the system backup browsed
root_ok() {
    case $1 in
        / | /proc | /proc/* | /sys | /sys/* | /dev | /dev/*) return 1 ;;
        "$MOUNTS"/*) [[ ${1#"$MOUNTS"/} =~ ^[A-Za-z0-9_.-]+$ ]] && mountpoint -q "$1" ;;
        "$BROWSE"/snapshots/*) [[ ${1#"$BROWSE"/snapshots/} =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}_[0-9]{6}$ ]] && mountpoint -q "$BROWSE" ;;
        "$MOUNTS" | /run/astroarch-backup | /run/astroarch-backup/*) return 1 ;;
    esac
}

# Is the root a read-only mount made by root, whose files nobody can change during the
# restore? An old disk mounted by `mount` with its device set read-only (not when a file
# manager had it mounted, maybe writable), a snapshot of the browse mount (read-only loop
# and mount, read-only btrfs snapshot). Anything else is read as the main user
root_trusted() {
    local opts
    case $1 in
        "$MOUNTS"/*)
            [[ -e $MOUNTS/.setro-${1#"$MOUNTS"/} ]] || return 1
            opts=$(findmnt -nro OPTIONS --mountpoint "$1") ;;
        "$BROWSE"/snapshots/*)
            opts=$(findmnt -nro OPTIONS --mountpoint "$BROWSE") ;;
        *) return 1 ;;
    esac
    [[ $opts == ro || $opts == ro,* ]]
}

# Inside a root, and only what the catalog holds there: the files of a home and the
# images, never a file of the old system itself (a system backup holds all of them).
# SRC_REAL: the path resolved now (no link in its folders), the only one used afterwards;
# SRC_ROOT: its root
source_ok() {
    local r dir name=${1%/}
    name=${name##*/}
    [[ -n $name && $name != . && $name != .. ]] || return 1
    dir=$(realpath -e "$(dirname "${1%/}")" 2>/dev/null) || return 1
    for r in "${ROOTS[@]}"; do
        if [[ $dir/$name == "$r"/home/* ]] || inside "$dir/$name" "$r/srv/astro-data"; then
            SRC_REAL=$dir/$name
            SRC_ROOT=$r
            return 0
        fi
    done
    return 1
}

# Runs a command reading in folder $2 (resolved), as root for a trusted root $1 else as
# the main user. The folder is entered first and checked to be that very path (no link
# swapped in on the way since the check)
read_in() {   # root folder command...
    local root=$1 dir=$2
    shift 2
    cd -P -- "$dir" 2> /dev/null && [[ $(pwd -P) == "$dir" ]] || { echo "changed since it was checked: $dir" >&2; return 1; }
    if [[ -n ${TRUSTED[$root]:-} ]]; then
        "$@"
    else
        runuser -u "$MAIN_USER" -- "$@"
    fi
}

# A path as a pattern of find -path that matches only itself
glob_escape() {
    local s=${1//\\/\\\\}
    s=${s//\*/\\*}
    s=${s//\?/\\?}
    printf '%s' "${s//\[/\\[}"
}

# Move a target aside, under the restore-DATE folder of its user
# and list it at once in restored.list: an undo after a restore cut short still finds it
set_aside() {   # user relhome
    make_aside "$1" \
        && runuser -u "$1" -- mkdir -p "${ASIDE[$1]}/files/$(dirname "$2")" \
        && runuser -u "$1" -- mv -T "$(home_of "$1")/$2" "${ASIDE[$1]}/files/$2" \
        && printf '%s\0' "$2" | runuser -u "$1" -- tee -a "${ASIDE[$1]}/restored.list" > /dev/null
}

# Images written by a restore cut short may be half files, which a new run would keep (it
# only adds what is missing): the list of what a copy writes stays next to it until the
# copy is complete, and a new run removes what it names first
PENDING=.astroarch-restore-pending
clean_pending() {   # user folder
    local f
    [[ -f $2/$PENDING ]] || return 0
    while IFS= read -r -d '' f; do
        [[ $f != /* && $f != *..* && $f != . ]] || continue
        [[ -f $2/${f#./} && ! -L $2/${f#./} ]] && runuser -u "$1" -- rm -f "$2/${f#./}"
    done < "$2/$PENDING"
    runuser -u "$1" -- rm -f "$2/$PENDING"
}

# Copy one entry: listed and read as read_in says (root or the main user), the user writes
copy_entry() {   # mode user source(resolved) target root
    local mode=$1 user=$2 src=$3 dst=$4 root=$5 home base top dbase list restored rel t ex s is_dir shared
    local skip=() prune=(-path '*/share/astroarch/restore-*')
    home=$(home_of "$user")
    if [[ -L $src ]]; then
        # A link of the old system (e.g. kiosk profiles shared with the main user)
        say "SKIPPED:link:$dst"
        return 0
    fi
    if [[ -d $src ]]; then
        base=$src top=. dbase=$dst
    else
        base=$(dirname "$src") top=${src##*/} dbase=$(dirname "$dst")
        [[ $top == "${dst##*/}" ]] || { say "SKIPPED:name:$dst"; return 0; }
    fi
    # Left out: the excluded paths, and the restore-DATE folders of the old system
    for ex in "${EXCLUDES[@]}"; do
        [[ $top == . && $ex == "$src"/* ]] && prune+=(-o -path "./$(glob_escape "${ex#"$src"/}")")
    done
    # The folder read is still the one checked (read_in checks it again for each use)
    (cd -P -- "$base" 2> /dev/null && [[ $(pwd -P) == "$base" ]]) || { say "ERROR:source:Changed since it was checked: $src"; return 1; }
    [[ $mode == data && -d $dbase ]] && clean_pending "$user" "$dbase"
    list=$(mktemp) && restored=$(mktemp) || return 1
    while IFS= read -r -d '' rel; do
        shared=0
        for s in "${skip[@]}"; do
            [[ $rel == "$s"/* ]] && { shared=1; break; }
        done
        ((shared)) && continue
        t=$dbase/${rel#./}
        [[ $rel == . ]] && t=$dbase
        # A link into a home of the old system: how its kiosk shared profiles, which the
        # Kiosk page sets here
        if [[ -L $base/$rel && $(readlink "$base/$rel") == /home/* ]]; then
            say "SKIPPED:link:$t"
            continue
        fi
        is_dir=0
        [[ -d $base/$rel && ! -L $base/$rel ]] && is_dir=1
        # A link here where the old system had none: left alone (the kiosk's profiles
        # shared with the main user are restored with the main user's)
        if [[ -L $t && ( $is_dir == 1 || ! -L $base/$rel ) ]]; then
            skip+=("$rel")
            say "SKIPPED:shared:$t"
            continue
        fi
        if [[ $mode == settings ]]; then
            # What is there is set aside (and listed): a file, or a file where a folder comes
            if [[ ( -e $t || -L $t ) && ( $is_dir == 0 || ! -d $t ) ]]; then
                set_aside "$user" "${t#"$home"/}" || { say "ERROR:aside:Cannot move $t aside"; rm -f "$list" "$restored"; return 1; }
            elif ((!is_dir)); then
                # A new file: listed before it is written, for `undo` to remove
                printf '%s\0' "${t#"$home"/}" >> "$restored"
            fi
            # The journal of a database goes with it: the old one would not match
            if [[ $t == *.sqlite ]]; then
                for s in -wal -shm -journal; do
                    [[ -e $t$s ]] || continue
                    set_aside "$user" "${t#"$home"/}$s" || { say "ERROR:aside:Cannot move $t$s aside"; rm -f "$list" "$restored"; return 1; }
                done
            fi
        fi
        # Images are only added: what is already here (file or folder) is left as it is
        [[ $mode == data && ( -e $t || -L $t ) ]] && continue
        printf '%s\0' "$rel" >> "$list"
    done < <(read_in "$root" "$base" find "$top" \( "${prune[@]}" \) -prune -o -print0)

    # Nothing left to copy (all shared with the main user, or already there)
    if [[ ! -s $list ]]; then
        rm -f "$list" "$restored"
        return 0
    fi
    if [[ -s $restored ]]; then
        make_aside "$user" && runuser -u "$user" -- tee -a "${ASIDE[$user]}/restored.list" < "$restored" > /dev/null \
            || { say "ERROR:aside:Cannot write the list of restored files"; rm -f "$list" "$restored"; return 1; }
    fi
    rm -f "$restored"
    runuser -u "$user" -- mkdir -p "$dbase" || { say "ERROR:write:Cannot create $dbase"; rm -f "$list"; return 1; }
    if [[ $mode == data ]]; then
        runuser -u "$user" -- tee "$dbase/$PENDING" < "$list" > /dev/null || { say "ERROR:write:Cannot write in $dbase"; rm -f "$list"; return 1; }
    fi
    # The names on stdin (the list file is root's): no recursion, links stored as links
    (read_in "$root" "$base" tar --null --no-recursion -T - -cf -) < "$list" \
        | runuser -u "$user" -- tar -C "$dbase" --no-overwrite-dir -xf -
    local rc=("${PIPESTATUS[@]}")
    rm -f "$list"
    if [[ ${rc[0]} != 0 || ${rc[1]} != 0 ]]; then
        say "ERROR:copy:Some files of $src could not be copied"
        return 1
    fi
    [[ $mode == data ]] && runuser -u "$user" -- rm -f "$dbase/$PENDING"
    # Images: group astro can write in what was added, as everywhere in the folder
    if [[ $mode == data && $dst == "$DATA"* ]]; then
        runuser -u "$user" -- find "$dst" -user "$user" ! -type l -exec chmod g+rwX {} + 2> /dev/null
        runuser -u "$user" -- find "$dst" -user "$user" -type d -exec chmod g+s {} + 2> /dev/null
    fi
    # PHD2 keeps the newest copy when the kiosk shares the profiles: this one
    [[ $dst == */.PHDGuidingV2 ]] && runuser -u "$user" -- touch -c "$dst"
    return 0
}

# The kiosk shares the main user's profiles: what was restored is opened to it again,
# by the kiosk's own helper (astroarch repository, scripts/astroarch-kiosk-setup.sh)
reshare_kiosk() {
    [[ -x $KIOSK_SETUP ]] || return 0
    [[ $(ASTROARCH_MAIN_USER=$MAIN_USER ASTROARCH_KIOSK_USER=$KIOSK_USER "$KIOSK_SETUP" status profiles 2> /dev/null) == shared ]] || return 0
    ASTROARCH_MAIN_USER=$MAIN_USER ASTROARCH_KIOSK_USER=$KIOSK_USER "$KIOSK_SETUP" apply --profiles shared > /dev/null \
        || say "SKIPPED:reshare:$KIOSK_USER"
}

cmd_restore() {
    local kind a b c line entries=() n i=0 failed=0 r
    check_not_busy
    while IFS= read -r line; do
        IFS=$'\t' read -r kind a b c <<< "$line"
        case $kind in
            label) LABEL=${a//[^[:print:]]/}; LABEL=${LABEL:0:200} ;;
            root) r=$(realpath -e "$a") || fail source "Not found: $a"
                  root_ok "$r" || fail source "Not a backup: $a"
                  if root_trusted "$r"; then
                      TRUSTED[$r]=1
                      # Held open until the end: the mount cannot be unmounted and swapped
                      exec {hold}< "$r" || fail source "Cannot open $a"
                  fi
                  ROOTS+=("$r") ;;
            exclude) EXCLUDES+=("$(realpath -m "$a")") ;;
            settings | data)
                [[ $a == "$MAIN_USER" || ( $a == "$KIOSK_USER" && -n $(home_of "$KIOSK_USER") ) ]] || fail user "Not a user of this restore: $a"
                # Not there (the page could not look into it): nothing to copy
                [[ -e $b || -L $b ]] || { say "SKIPPED:missing:$c"; continue; }
                source_ok "$b" || fail source "Not inside the backup: $b"
                target_ok "$a" "$c" || { say "SKIPPED:target:$c"; continue; }
                entries+=("$kind"$'\t'"$a"$'\t'"$SRC_REAL"$'\t'"$c"$'\t'"$SRC_ROOT") ;;
            "") ;;
            *) fail usage "Unknown line: $kind" ;;
        esac
    done
    ((${#ROOTS[@]})) || fail usage "No root"
    n=${#entries[@]}
    say "STAGE:copy"
    for line in "${entries[@]}"; do
        i=$((i + 1))
        say "PROGRESS:$i:$n"
        IFS=$'\t' read -r kind a b c r <<< "$line"
        copy_entry "$kind" "$a" "$b" "$c" "$r" || failed=1
    done
    reshare_kiosk
    for a in "${!ASIDE[@]}"; do
        say "ASIDE:${ASIDE[$a]}"
    done
    ((failed)) && exit 1
    say "STAGE:done"
}

# Put back what one restore set aside, for its user: the restored files are removed (the
# newest first, so a folder that replaced a file is empty when the file comes back), what
# they replaced goes back. A folder whose put-back failed is kept, with what is left to do
cmd_undo() {
    local dir user home rel t real i failed=0 rels left
    check_not_busy
    for dir in "$@"; do
        real=$(realpath -e "$dir") || fail folder "Not found: $dir"
        user=""
        for u in "$MAIN_USER" "$KIOSK_USER"; do
            home=$(home_of "$u")
            [[ -n $home && $(dirname "$real") == "$(realpath -m "$home/.local/share/astroarch")" ]] && { user=$u; break; }
        done
        [[ -n $user && ${real##*/} =~ ^restore-[0-9]{8}-[0-9]{6}$ ]] || fail folder "Not a restore folder: $dir"
        rels=()
        [[ -f $real/restored.list ]] && mapfile -d '' -t rels < "$real/restored.list"
        declare -A done_rel=()
        left=()
        for ((i = ${#rels[@]} - 1; i >= 0; i--)); do
            rel=${rels[i]}
            # Listed twice (a database journal set aside, then restored): once is enough
            [[ -n $rel && -z ${done_rel[$rel]:-} ]] || continue
            done_rel[$rel]=1
            [[ $rel != /* && $rel != *..* ]] || continue
            t=$home/$rel
            if [[ -d $t && ! -L $t ]]; then
                # A folder that replaced a file: only once empty (never what was added since;
                # the empty folders the restore made in it go)
                runuser -u "$user" -- find "$t" -mindepth 1 -depth -type d -empty -delete 2> /dev/null
                [[ -e $real/files/$rel || -L $real/files/$rel ]] && ! runuser -u "$user" -- rmdir "$t" 2> /dev/null \
                    && { left+=("$rel"); say "SKIPPED:putback:$t"; continue; }
            else
                runuser -u "$user" -- rm -f "$t"
            fi
            if [[ -e $real/files/$rel || -L $real/files/$rel ]]; then
                runuser -u "$user" -- mkdir -p "$(dirname "$t")" \
                    && runuser -u "$user" -- mv -T "$real/files/$rel" "$t" \
                    || { left+=("$rel"); say "SKIPPED:putback:$t"; }
            fi
        done
        runuser -u "$user" -- touch -c "$home/.PHDGuidingV2" 2> /dev/null
        if ((${#left[@]})); then
            # Kept: only what is still to put back, so a new undo removes nothing else
            printf '%s\0' "${left[@]}" | runuser -u "$user" -- tee "$real/restored.list" > /dev/null
            say "KEPT:$real"
            failed=1
        else
            runuser -u "$user" -- rm -rf "$real" || say "SKIPPED:cleanup:$real"
        fi
        unset done_rel
    done
    reshare_kiosk
    ((failed)) && fail putback "Some files could not be put back"
    say "STAGE:done"
}

# Closing the Control Center window does not stop the helper half-way
trap '' PIPE HUP

[[ $EUID == 0 ]] || fail root "Must run as root"
[[ -n $MAIN_USER && $MAIN_USER != root && -n $(home_of "$MAIN_USER") ]] || fail user "Unknown user"
mkdir -p "$MOUNTS"
exec 9> "$MOUNTS/.lock"
flock -n 9 || fail running "Already running"

case ${1:-} in
    mount)   cmd_mount "${2:-}" ;;
    unmount) cmd_unmount ;;
    restore) cmd_restore ;;
    undo)    shift; (($#)) || fail usage "undo FOLDER..."; cmd_undo "$@" ;;
    *)       fail usage "astroarch-restore-helper mount DEVICE | unmount | restore | undo FOLDER..." ;;
esac

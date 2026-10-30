#!/bin/bash
# astroarch-control-center-probe NAME
#
# Reads one piece of state of the running system for the Control Center, as the
# user running it. Never changes anything.
#
# Output: one line of text on stdout (the value shown on the page), nothing when
# there is no value. Exit code 0 = "yes / found", anything else = "no / not found".
# A probe that takes more than a few seconds counts as "no": the Control Center
# kills its process group. A command that leaves the group (setsid, timeout without
# --foreground) must never write to stdout: it would keep a reader thread of the
# Control Center waiting until it exits.

set -u
export LC_ALL=C

CONFIG=${XDG_CONFIG_HOME:-$HOME/.config}
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/astroarch

# The main user: this one, or the one whose kiosk user this is (<user>-kiosk), as the
# kiosk scripts of the astroarch repository name them; its home holds the AstroArch
# repository. ASTROARCH_MAIN_USER replaces it (tests)
ME=$(id -un)
MAIN_USER=${ASTROARCH_MAIN_USER:-${ME%-kiosk}}
main_home() { getent passwd "$MAIN_USER" | cut -d: -f6; }

# Default passwords of the image: astroarch_build.sh (users and Samba) and
# scripts/create_ap.sh (hotspot) in the astroarch repository
DEFAULT_USER_PASSWORD=astro
DEFAULT_HOTSPOT_PASSWORD=astronomy

# Active connection used to reach the network: Ethernet, or Wi-Fi as a client
# (not the field hotspot, not VPNs or bridges). Prints "name<TAB>device".
# NAME is the last field: nmcli -t writes a ":" inside it as "\:"
home_connection() {
    nmcli -t -f UUID,TYPE,DEVICE,NAME connection show --active 2>/dev/null | {
        while IFS=: read -r uuid type device name; do
            name=${name//\\:/:}
            case $type in
                802-3-ethernet) ;;
                802-11-wireless)
                    [[ $(nmcli -g 802-11-wireless.mode connection show "$uuid") == ap ]] && continue ;;
                *) continue ;;
            esac
            printf '%s\t%s\n' "$name" "$device"
            exit 0
        done
        exit 1
    }
}

# Wi-Fi connection in access-point mode (the field hotspot). Prints "uuid<TAB>ssid".
# Returns 1 when there is none, 2 when NetworkManager does not answer
hotspot_connection() {
    local list uuid type
    list=$(nmcli -t -f UUID,TYPE connection show 2>/dev/null) || return 2
    while IFS=: read -r uuid type; do
        [[ $type == 802-11-wireless ]] || continue
        [[ $(nmcli -g 802-11-wireless.mode connection show "$uuid") == ap ]] || continue
        printf '%s\t%s\n' "$uuid" "$(nmcli -g 802-11-wireless.ssid connection show "$uuid")"
        return 0
    done <<< "$list"
    return 1
}

# State of a system service: "on" (running), "off", or "missing" (not installed)
service_state() {
    if [[ -z $(systemctl list-unit-files --no-legend "$1.service" 2>/dev/null) ]]; then
        echo missing
    elif systemctl is-active --quiet "$1.service"; then
        echo on
    else
        echo off
    fi
}

# Value of KEY in [GROUP] of an ini-style FILE
ini_value() {
    awk -F= -v group="[$2]" -v key="$3" '
        $0 == group { inside = 1; next }
        /^\[/ { inside = 0 }
        inside && $1 == key { sub(/^[^=]*=/, ""); print; exit }' "$1" 2>/dev/null
}

case ${1:-} in
    # ---------------------------------------------------------------- network
    network-ok)
        home_connection >/dev/null ;;
    network-home)
        line=$(home_connection) || exit 1
        name=${line%%$'\t'*}
        device=${line#*$'\t'}
        ip=$(nmcli -g IP4.ADDRESS device show "$device" 2>/dev/null | head -n1 | cut -d/ -f1)
        echo "$name${ip:+ · $ip}" ;;
    hotspot)
        line=$(hotspot_connection) || exit 1
        uuid=${line%%$'\t'*}
        ssid=${line#*$'\t'}
        if nmcli -t -f UUID connection show --active | grep -qx "$uuid"; then
            echo "$ssid · on"
        else
            echo "$ssid · off"
        fi ;;
    hotspot-uuid)
        line=$(hotspot_connection) || exit 1
        echo "${line%%$'\t'*}" ;;
    host)
        echo "$(hostname).local" ;;

    # -------------------------------------------------------------- passwords
    # "default", "changed", or "none" (no such password for this user); no answer
    # when it cannot be read. The password itself is never printed
    #
    # User: pam's unix_chkpwd checks a password of the calling user only (setuid helper
    # of pam_unix). A wrong guess is logged, so the answer is kept until /etc/shadow
    # changes (its date can be read by everyone)
    password-user)
        shadow_date=$(stat -c %Y /etc/shadow 2>/dev/null)
        [[ -n $shadow_date && -x /usr/bin/unix_chkpwd ]] || exit 1
        cached=$(cat "$CACHE/password-user" 2>/dev/null)
        if [[ ${cached% *} == "$shadow_date" ]]; then
            echo "${cached#* }"
            exit 0
        fi
        if printf '%s\0' "$DEFAULT_USER_PASSWORD" | /usr/bin/unix_chkpwd "$(id -un)" nullok 2>/dev/null; then
            answer=default
        else
            answer=changed
        fi
        mkdir -p "$CACHE" && echo "$shadow_date $answer" > "$CACHE/password-user"
        echo "$answer" ;;
    # Samba (shared folders): a log-in to the local server, on the IPC$ share only.
    # A user Samba does not know is let in as a guest whatever the password, so a
    # random password must be refused first. Failed log-ins are not logged by default
    password-samba)
        user=$(id -un)
        # 0 = let in, 1 = wrong password, 2 = no answer (server down, not installed...)
        smb_login() {
            local out
            out=$(smbclient -s /dev/null //localhost/IPC\$ -m SMB3 -U "$user%$1" -c exit 2>&1) && return 0
            [[ $out == *LOGON_FAILURE* ]] && return 1
            return 2
        }
        smb_login "not-a-password-$RANDOM$RANDOM"
        case $? in
            0) echo none; exit 0 ;;
            2) exit 1 ;;
        esac
        smb_login "$DEFAULT_USER_PASSWORD"
        case $? in
            0) echo default ;;
            1) echo changed ;;
            *) exit 1 ;;
        esac ;;
    # Hotspot: NetworkManager gives its key to the user of the session
    password-hotspot)
        line=$(hotspot_connection)
        case $? in
            1) echo none; exit 0 ;;
            2) exit 1 ;;
        esac
        psk=$(nmcli -s -g 802-11-wireless-security.psk connection show "${line%%$'\t'*}" 2>/dev/null) || exit 1
        [[ -n $psk ]] || exit 1
        [[ $psk == "$DEFAULT_HOTSPOT_PASSWORD" ]] && echo default || echo changed ;;

    # --------------------------------------------------------------- language
    # Locale code (en_US): the interface language of this user in Plasma, else its
    # formats, else the system's
    language)
        lang=$(ini_value "$CONFIG/plasma-localerc" Translations LANGUAGE)
        lang=${lang%%:*}
        [[ -n $lang ]] || lang=$(ini_value "$CONFIG/plasma-localerc" Formats LANG)
        [[ -n $lang ]] || lang=$(sed -n 's/^LANG=//p' /etc/locale.conf 2>/dev/null)
        [[ -n $lang ]] || exit 1
        echo "$lang" ;;
    # Keyboard layouts of this user, else the system's; "default" when none is set
    keyboard)
        layouts=$(ini_value "$CONFIG/kxkbrc" Layout LayoutList)
        [[ -n $layouts ]] || layouts=$(localectl status 2>/dev/null | sed -n 's/^ *X11 Layout: //p' | grep -v unset)
        if [[ -z $layouts ]]; then
            echo "default"
            exit 0
        fi
        first=${layouts%%,*}
        name=$(awk -v l="$first" '/^! layout/ { f = 1; next } /^!/ { f = 0 }
            f && $1 == l { sub(/^ *[^ ]+ +/, ""); print; exit }' /usr/share/X11/xkb/rules/base.lst)
        echo "${name:-$first}" ;;
    # "zone|UTC offset" (Europe/Rome|+0100): the offset gives the local time of the
    # zone even when it changed after the Control Center started
    timezone)
        zone=$(timedatectl show -p Timezone --value 2>/dev/null)
        [[ -n $zone ]] || exit 1
        echo "$zone|$(TZ=$zone date +%z)" ;;

    # ------------------------------------------------------- location (KStars)
    # "City|latitude|longitude" when KStars has a site
    location)
        rc=$CONFIG/kstarsrc
        city=$(ini_value "$rc" Location CityName)
        lat=$(ini_value "$rc" Location Latitude)
        lon=$(ini_value "$rc" Location Longitude)
        [[ -n $city || -n $lat ]] || exit 1
        echo "$city|$lat|$lon" ;;

    # ------------------------------------------------------ equipment profiles
    # "count|first name" of the Ekos profiles other than the simulators. Opened
    # read-only: a check must never create the database
    ekos-profiles)
        db=$DATA/kstars/userdb.sqlite
        [[ -s $db ]] && command -v sqlite3 >/dev/null || exit 1
        out=$(sqlite3 "file:$db?mode=ro" \
            "select count(*), coalesce(min(name), '') from profile where name not like 'Simulator%'" 2>/dev/null) || exit 1
        count=${out%%|*}
        [[ $count -gt 0 ]] || exit 1
        echo "$out" ;;
    # Name of the first PHD2 profile (smallest number) with a real camera and a real
    # mount chosen: [profile/N] holds name=, [profile/N/camera] and [profile/N/scope]
    # hold LastMenuchoice= (the case of the key varies); simulators do not count
    phd2-profile)
        cfg=$HOME/.PHDGuidingV2
        [[ -s $cfg ]] || exit 1
        awk -F= '
            /^\[/ {
                p = ""; s = ""
                if (match($0, /^\[profile\/[0-9]+(\/(camera|scope))?\]$/)) {
                    sect = substr($0, 2, length($0) - 2)
                    n = split(sect, part, "/")
                    p = part[2] + 0
                    s = (n == 3) ? part[3] : "main"
                }
                next
            }
            p == "" { next }
            s == "main" && $1 == "name" { name[p] = substr($0, index($0, "=") + 1) }
            s != "main" && tolower($1) == "lastmenuchoice" {
                v = substr($0, index($0, "=") + 1)
                if (v != "" && v != "None" && tolower(v) !~ /simulator/) has[p, s] = 1
            }
            END {
                best = -1
                for (q in name) if (has[q, "camera"] && has[q, "scope"] && (best < 0 || q + 0 < best)) best = q + 0
                if (best < 0) exit 1
                print name[best]
            }' "$cfg" ;;
    equipment-ok)
        "$0" ekos-profiles >/dev/null && "$0" phd2-profile >/dev/null ;;

    # ------------------------------------------------------------------ update
    # The AstroArch repository is in the home of the main user: the kiosk user has
    # none of its own
    version)
        for home in "$HOME" "$(main_home)"; do
            cat "$home/.astroarch/configs/.astroarch.version" 2>/dev/null | grep . && exit 0
        done
        exit 1 ;;
    # update-astroarch is a zsh function from ~/.zshrc, not a program. Read in the
    # file: starting an interactive zsh (oh-my-zsh, the motd) takes seconds on a Pi
    update-command)
        grep -qE '^[[:space:]]*(function[[:space:]]+)?update-astroarch[[:space:]]*\(\)' "$HOME/.zshrc" 2>/dev/null ;;
    # Exit 0 and print the announcement while the AstroArch news ask not to update.
    # The feed has only a summary and a body: a notice about X11 only does not apply
    # to a system that already runs Wayland
    update-hold)
        summary=$(curl -fsS -m 4 http://astroarch.astromatto.com:9000/notifications.json 2>/dev/null \
            | grep -io '"summary" *: *"[^"]*do not update[^"]*"' | head -n1 \
            | sed 's/^"summary" *: *"//; s/"$//')
        [[ -n $summary ]] || exit 1
        if [[ ${summary,,} == *x11* && ${summary,,} != *wayland* && ${XDG_SESSION_TYPE:-} == wayland ]]; then
            exit 1
        fi
        echo "$summary" ;;

    # --------------------------------------------------------------- services
    # The functions of the AstroArch zsh plugins (gps_on, bluetooth_on...) are loaded by
    # the AstroArch ~/.zshrc
    astroarch-shell)
        grep -q '\.astroarch/plugins' "$HOME/.zshrc" 2>/dev/null ;;
    gps)
        service_state gpsd ;;
    # The plugins folder of the main user's AstroArch repository (Services page)
    plugins-dir)
        dir=$(main_home)/.astroarch/plugins
        [[ -d $dir ]] || exit 1
        echo "$dir" ;;
    # Full current on the USB ports: "none" on anything but a Raspberry Pi 5
    usb-current)
        if ! grep -qa 'Raspberry Pi 5' /proc/device-tree/model 2>/dev/null; then
            echo none
        elif grep -q '^usb_max_current_enable=1' /boot/config.txt 2>/dev/null; then
            echo on
        else
            echo off
        fi ;;

    *)
        echo "unknown probe: ${1:-}" >&2
        exit 2 ;;
esac

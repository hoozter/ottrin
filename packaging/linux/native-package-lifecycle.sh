#!/bin/sh
# Native package transactions own /usr/bin/ottrin-indexd, not portable services
# or users' search data. All user-file operations run with that user's identity.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
unit=ottrin-index.service
native_exec='ExecStart="/usr/bin/ottrin-indexd" --index-daemon'

control() {
    if [ "$scope" = user ]; then systemctl --user "$@"; else systemctl "$@"; fi
}

user_manager_mode() {
    if [ ! -d /run/systemd/system ]; then
        echo offline
    elif systemctl is-active --quiet "user@$(id -u).service"; then
        echo online
    else
        code=$?
        case "$code" in
            3|4) echo offline ;;
            *) echo 'Cannot inspect the owning user manager' >&2; return 1 ;;
        esac
    fi
}

service_operation() (
    operation=$1
    manager=$2
    [ "$operation:$manager" != refresh:offline ] || return 0
    if [ "$scope" = user ]; then
        fragment=$HOME/.config/systemd/user/$unit
    else
        fragment=/etc/systemd/system/$unit
    fi
    if [ "$manager" = online ]; then
        fragment=$(control show --property=FragmentPath --value "$unit")
        [ -n "$fragment" ] || return 0
    fi
    [ -f "$fragment" ] || return 0
    # Only the authoritative generated native entry is ours. Never execute or
    # interpret a unit file as shell, and never remove another package's unit.
    grep -Fqx "$native_exec" "$fragment" || return 0
    [ "$(grep -c '^ExecStart=' "$fragment")" -eq 1 ] || {
        echo "Cannot establish native service ownership: $fragment" >&2; return 1;
    }
    if [ "$manager" = online ]; then
        effective=$(control show --property=ExecStart --value "$unit")
        case "$effective" in
            '{ path=/usr/bin/ottrin-indexd ; '*) ;;
            *) return 0 ;;
        esac
    else
        # Without a manager we cannot resolve an overriding ExecStart. Refuse
        # removal instead of guessing which service would start at next login.
        if [ "$scope" = user ]; then
            set -- "$fragment.d" "$HOME/.config/systemd/user/$unit.d" \
                "$HOME/.local/share/systemd/user/$unit.d" "/etc/systemd/user/$unit.d" \
                "/usr/lib/systemd/user/$unit.d"
        else
            set -- "$fragment.d" "/etc/systemd/system/$unit.d" "/usr/lib/systemd/system/$unit.d"
        fi
        for directory do
            for override in "$directory"/*.conf; do
                if [ -e "$override" ]; then
                    echo "Cannot resolve offline native service override: $override" >&2
                    return 1
                fi
            done
        done
        if [ "$scope" = user ]; then
            target=default.target
            enablement=$HOME/.config/systemd/user/$target.wants/$unit
        else
            target=multi-user.target
            enablement=/etc/systemd/system/$target.wants/$unit
        fi
        if ! grep -Fqx "WantedBy=$target" "$fragment" \
            || [ "$(grep -c '^WantedBy=' "$fragment")" -ne 1 ] \
            || grep -Eq '^(Alias|Also|RequiredBy|UpheldBy)=' "$fragment"; then
            echo "Cannot establish offline native enablement ownership: $fragment" >&2
            return 1
        fi
        if [ -e "$enablement" ] || [ -L "$enablement" ]; then
            if [ ! -L "$enablement" ] \
                || [ "$(readlink -f -- "$enablement")" != "$(readlink -f -- "$fragment")" ]; then
                echo "Cannot remove unrelated offline enablement: $enablement" >&2
                return 1
            fi
        fi
    fi
    case "$operation:$manager" in
        check:*) ;;
        refresh:online)
            control daemon-reload
            # Preserve stopped/disabled services; only replace a running daemon.
            control try-restart "$unit"
            ;;
        refresh:offline) ;;
        remove:online)
            control disable --now "$unit"
            rm -- "$fragment"
            control daemon-reload
            ;;
        remove:offline)
            # There is no user bus. Only unlink the generated, identity-checked
            # enablement; never follow it or recursively remove user files.
            if [ -L "$enablement" ]; then rm -- "$enablement"; fi
            rm -- "$fragment"
            ;;
        *) echo 'Invalid native package user operation' >&2; return 2 ;;
    esac
)

case "${1-}" in
    user-remove|user-refresh|user-check)
        [ "$#" -eq 1 ] || exit 2
        scope=user
        # Inspect after runuser's PAM session setup: it may itself start a
        # manager that was offline when the package transaction began.
        manager=$(user_manager_mode)
        if [ "$1" = user-remove ]; then
            /usr/bin/ottrin-indexd --native-package-stop
        fi
        service_operation "${1#user-}" "$manager"
        ;;
    remove|refresh)
        [ "$#" -eq 1 ] || exit 2
        [ "$(id -u)" -eq 0 ] || { echo 'Native package lifecycle requires root' >&2; exit 1; }
        operation=$1
        self=$(readlink -f -- "$0")
        accounts=$(getent passwd)
        # Resolve every standard registration before changing any of them.
        # In particular an offline override must reject removal up front.
        phases=$operation
        if [ "$operation" = remove ]; then phases='check remove'; fi
        for phase in $phases; do
        # A here-document keeps failures in this shell, rather than losing the
        # transaction result in a pipeline subshell.
        while IFS=: read -r account unused uid gid gecos account_home login_shell; do
            case "$uid" in ''|*[!0-9]*) echo 'Invalid account uid' >&2; exit 1 ;; esac
            case "$account_home" in /*) ;; *) continue ;; esac
            runuser -u "$account" -- env -i PATH="$PATH" HOME="$account_home" \
                USER="$account" LOGNAME="$account" XDG_RUNTIME_DIR="/run/user/$uid" \
                DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" \
                "$self" "user-$phase"
        done <<EOF
$accounts
EOF
        scope=system
        unit=ottrin-administrator-index.service
        native_exec='ExecStart="/usr/bin/ottrin-indexd" --administrator-index-daemon'
        manager=offline
        [ ! -d /run/systemd/system ] || manager=online
        service_operation "$phase" "$manager"
        done
        ;;
    *) echo 'Usage: native-package-lifecycle.sh remove|refresh' >&2; exit 2 ;;
esac

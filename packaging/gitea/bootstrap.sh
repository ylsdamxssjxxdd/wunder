#!/bin/sh
set -eu
umask 077

mkdir -p /bootstrap
gitea_cli() {
    /sbin/su-exec git /usr/local/bin/gitea --config /data/gitea/conf/app.ini "$@"
}

if [ ! -f /bootstrap/admin-created ]; then
    gitea_cli admin user list > /bootstrap/users.tmp
    if awk -v username="$GITEA_ADMIN_USER" 'NR > 1 && $2 == username { found=1 } END { exit !found }' /bootstrap/users.tmp; then
        printf '%s\n' 'Administrator already exists; its password is unchanged.'
    else
        password="$(head -c 36 /dev/urandom | base64 | tr -d '\n')"
        printf '%s\n' "$password" > /bootstrap/admin-password
        gitea_cli admin user create --username "$GITEA_ADMIN_USER" \
            --email "$GITEA_ADMIN_EMAIL" --password "$password" --admin \
            --must-change-password=true > /bootstrap/create-user.log 2>&1
        unset password
        printf '%s\n' 'Administrator created. Read data/bootstrap/admin-password locally, then change it at first login.'
    fi
    rm -f /bootstrap/users.tmp
    touch /bootstrap/admin-created
fi

if [ ! -s /bootstrap/runner-token ]; then
    gitea_cli actions generate-runner-token > /bootstrap/token-output.tmp
    token="$(tail -n 1 /bootstrap/token-output.tmp | tr -d '\r\n')"
    case "$token" in
        ''|*[!a-zA-Z0-9_-]*) printf '%s\n' 'Unable to generate a valid runner registration token.' >&2; exit 1 ;;
    esac
    printf '%s' "$token" > /bootstrap/runner-token.tmp
    mv /bootstrap/runner-token.tmp /bootstrap/runner-token
    rm -f /bootstrap/token-output.tmp
    unset token
fi
printf '%s\n' 'Offline initialization complete; runner registration is automatic.'

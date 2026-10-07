#!/bin/sh
set -eu
task_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
task_output=${1:-"$task_root/.env"}
if [ -e "$task_output" ]; then
    printf '%s\n' 'Existing environment file preserved. No secrets were changed.'
    exit 0
fi
task_random() {
    if command -v openssl >/dev/null 2>&1; then
        openssl rand -hex 32
    else
        od -An -N32 -tx1 /dev/urandom | tr -d ' \n'
    fi
}
task_postgres=$(task_random)
task_session=$(task_random)
task_internal=$(task_random)
task_admin=$(task_random)
(
    umask 077
    set -C
    sed \
        -e "s/^POSTGRES_PASSWORD=.*/POSTGRES_PASSWORD=$task_postgres/" \
        -e "s/^SESSION_SECRET=.*/SESSION_SECRET=$task_session/" \
        -e "s/^INTERNAL_SERVICE_KEY=.*/INTERNAL_SERVICE_KEY=$task_internal/" \
        -e "s/^BOOTSTRAP_ADMIN_PASSWORD=.*/BOOTSTRAP_ADMIN_PASSWORD=$task_admin/" \
        "$task_root/.env.example" > "$task_output"
)
printf '%s\n' 'Created an environment file with independent random secrets. Read its initial administrator credentials locally.'
printf '%s\n' 'Local start: docker compose up -d --build --wait'

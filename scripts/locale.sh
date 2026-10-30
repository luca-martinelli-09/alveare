#!/bin/sh
# Alveare in locale: pannello su http://pannello.localhost:8080, siti su http://<nome>.localhost:8080.
#   scripts/locale.sh up      prepara il .env (se manca), costruisce le immagini e avvia
#   scripts/locale.sh pass    password temporanea dell'admin (primo accesso)
#   scripts/locale.sh logs    log del pannello
#   scripts/locale.sh down    ferma tutto (i dati restano in data/)
#   scripts/locale.sh reset   ferma e cancella data/ (siti, database, backup)
# Con PROFILI="workers php84" si accendono anche worker e PHP 8.4.
set -eu
cd "$(dirname "$0")/.."
port=${ALVEARE_LOCAL_PORT:-8080}
profiles=""
for p in ${PROFILI:-}; do profiles="$profiles --profile $p"; done
# shellcheck disable=SC2086
dc() { docker compose -p alveare-locale -f compose.yml -f compose.local.yml $profiles "$@"; }

write_env() {
    rnd() { head -c 32 /dev/urandom | base64 | tr -d '\n'; }
    hex() { head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
    runtimes=""
    case " ${PROFILI:-} " in *" php84 "*) runtimes="8.4" ;; esac
    cat > .env <<ENV
# alveare-locale: generato da scripts/locale.sh, solo per provare in locale.
ALVEARE_SECRET_KEY=$(rnd)
MARIADB_ROOT_PASSWORD=$(hex)
ALVEARE_ADMIN_EMAIL=admin@alveare.localhost
ALVEARE_PANEL_HOST=pannello.localhost
ALVEARE_ADMINER_HOST=adminer.localhost
ALVEARE_PANEL_URL=http://pannello.localhost:$port
ALVEARE_SITE_URL=http://{domain}:$port
ALVEARE_INSECURE_COOKIE=1
ALVEARE_PHP_RUNTIMES=$runtimes
RESTIC_REPOSITORY=/srv/backups/repo
RESTIC_PASSWORD=$(hex)
ALVEARE_METRICS_TOKEN=$(hex)
ENV
    echo ".env locale creato"
}

case "${1:-up}" in
up)
    if [ ! -f .env ] || grep -q '^ALVEARE_SECRET_KEY=x$' .env; then
        write_env
    elif ! grep -q '^# alveare-locale' .env; then
        echo "c'è già un .env che non è quello locale: lo lascio stare (spostalo se vuoi provare in locale)"
        exit 1
    fi
    dc up -d --build --wait
    echo
    echo "pannello: http://pannello.localhost:$port  (utente admin@alveare.localhost)"
    echo "password temporanea: scripts/locale.sh pass"
    echo "crea un sito con un dominio come prova.localhost e aprilo su http://prova.localhost:$port"
    ;;
pass) dc logs panel | sed -n 's/.*password temporanea \([^ ]*\).*/\1/p' | tail -1 ;;
logs) dc logs -f panel ;;
down) dc down ;;
reset)
    dc down -v
    # I file appartengono a root e agli utenti dei siti: si cancellano da un container.
    docker run --rm -v "$PWD/data:/data" debian:trixie-slim find /data -mindepth 1 -delete
    echo "data/ svuotata"
    ;;
*) sed -n '2,9p' "$0"; exit 1 ;;
esac

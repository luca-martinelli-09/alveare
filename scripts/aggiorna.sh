#!/bin/sh
# Aggiorna pannello e immagine PHP a una versione (tag delle immagini, mai latest).
# All'avvio il pannello copia panel.db (data/panel/panel-<vecchia>-<data>.db) prima delle
# migrazioni e poi fa un backup restic: per tornare indietro c'è scripts/torna-a.sh.
# Uso: scripts/aggiorna.sh v0.3.0
set -eu
cd "$(dirname "$0")/.."
new=${1:?uso: scripts/aggiorna.sh <versione>}
old=$(sed -n 's/^ALVEARE_VERSION=//p' .env)
[ "$new" != "latest" ] || { echo "mai latest: indica una versione"; exit 1; }
echo "aggiornamento da ${old:-dev} a $new"
if grep -q '^ALVEARE_VERSION=' .env; then
    sed -i "s/^ALVEARE_VERSION=.*/ALVEARE_VERSION=$new/" .env
else
    echo "ALVEARE_VERSION=$new" >> .env
fi
docker compose pull panel php
docker compose up -d --wait
echo "fatto. Se qualcosa non va: scripts/torna-a.sh ${old:-dev}"

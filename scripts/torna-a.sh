#!/bin/sh
# Torna a una versione precedente: immagini di quella versione e la copia di panel.db fatta
# dal pannello prima delle migrazioni. Le migrazioni sono solo additive, ma lo stato salvato
# dopo l'aggiornamento (siti nuovi, modifiche) va perso: il pannello lo dice prima di farlo.
# Uso: scripts/torna-a.sh v0.2.0
set -eu
cd "$(dirname "$0")/.."
old=${1:?uso: scripts/torna-a.sh <versione>}
plain=${old#v}
copy=$(ls -1t data/panel/panel-"$plain"-*.db data/panel/panel-"$old"-*.db 2>/dev/null | head -1 || true)
[ -n "$copy" ] || { echo "nessuna copia di panel.db per $old in data/panel"; exit 1; }
echo "ripristino $copy e immagini $old: le modifiche fatte dopo l'aggiornamento si perdono."
printf "continuare? [s/N] "
read -r ok
[ "$ok" = "s" ] || exit 1
docker compose stop panel
cp data/panel/panel.db "data/panel/panel-scartato-$(date +%s).db"
cp "$copy" data/panel/panel.db
rm -f data/panel/panel.db-wal data/panel/panel.db-shm
sed -i "s/^ALVEARE_VERSION=.*/ALVEARE_VERSION=$old/" .env
echo "$plain" > data/panel/VERSION
docker compose up -d --wait
echo "tornato a $old"

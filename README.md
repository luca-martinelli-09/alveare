# Alveare

Pannello self-hosted per ospitare molti siti WordPress e PHP su un solo server, **senza un container per sito**. Gira tutto in Docker, su ARM64 e AMD64. La specifica completa è in [docs/spec.md](docs/spec.md).

```
cloudflared ──► caddy ──► php (un pool FPM per sito, utente Unix dedicato)
                  │            │
                  └── panel ───┼── agente PHP (comandi in whitelist)
                         │     ├── mariadb (un database e un utente per sito)
                         │     └── redis   (un utente ACL per sito)
                         └── restic ──► storage S3 (R2)
```

I container sono fissi e non dipendono dal numero di siti: `panel`, `php`, `caddy`, `mariadb`, `redis` e i connettori `cloudflared` (fuori dal compose). Facoltativi, con un profilo del compose: `php-workers` (code e Horizon delle app Laravel), `php84` (un'altra versione di PHP) e `php-canary` (una nuova immagine PHP provata su un sito). Tutto ciò che serve a un sito si rigenera dallo stato SQLite del pannello: Caddyfile, pool FPM, utenti Unix, ACL di Redis, worker.

## Installazione

```bash
git clone <repo> alveare && cd alveare
cp .env.example .env        # compila chiave, password MariaDB, email admin, restic
docker network create cloudflared   # se non esiste già quella dei connettori
docker compose up -d --build
docker compose logs panel | grep "password temporanea"
```

Poi:

1. Nel tunnel Cloudflare, punta l'hostname di `ALVEARE_PANEL_HOST` (e di `ALVEARE_ADMINER_HOST`) a `http://caddy:80`, protetti da Cloudflare Access.
2. Accedi con l'email dell'admin e la password temporanea: al primo accesso cambi la password e attivi il TOTP (salvo `ALVEARE_TOTP=false` nel `.env`). Dal profilo puoi aggiungere una passkey.
3. **Scarica il kit di ripristino** (Account → Kit di ripristino) e conservalo fuori dal server.
4. Crea un sito. Con un account Cloudflare configurato (`CLOUDFLARE_<ETICHETTA>_*`) l'hostname va da solo sul tunnel con il suo CNAME; senza, aggiungilo a mano verso `http://caddy:80`.

Funzioni facoltative, tutte dal `.env` (vedi `.env.example`):

| Cosa | Come |
| --- | --- |
| Inviti e reset della password via email | `PANEL_SMTP_FROM` (+ credenziali SES dei siti o `PANEL_SMTP_*`) |
| Avvisi su Telegram | `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID` |
| Tunnel e regole WAF/rate limit via API | `CLOUDFLARE_<ETICHETTA>_API_TOKEN`, `_ACCOUNT_ID`, `_TUNNEL_ID` |
| Metriche per Grafana | `ALVEARE_METRICS_TOKEN`, poi Prometheus su `/metrics`; dashboard in `docs/grafana/alveare.json` |
| Altre versioni di PHP | `ALVEARE_PHP_RUNTIMES=8.4` e `docker compose --profile php84 up -d` |
| Immagine PHP in prova | `ALVEARE_CANARY_VERSION=<versione>`, `ALVEARE_PHP_RUNTIMES=canary`, profilo `canary`; si sposta un sito di prova e poi tutti |
| App Laravel (worker, deploy da git) | template `laravel`, `docker compose --profile workers up -d` |

## Aggiornare il pannello

```bash
scripts/aggiorna.sh v0.3.0
```

All'avvio della nuova versione il pannello copia `panel.db` (in `data/panel/panel-<vecchia>-<data>.db`) prima delle migrazioni, che sono solo additive, e fa un backup restic. Se qualcosa non va, `scripts/torna-a.sh v0.2.0` rimette le immagini e la copia del database.

## CLI

La CLI `alveare` (binari nelle release per Linux, macOS e Windows) usa la stessa API con un token personale, quindi ha i permessi dell'utente:

```bash
alveare login --url https://pannello.esempio.it --token alv_...
alveare sites list
alveare wp sona.it -- plugin list
alveare backup run sona.it
alveare logs tail sona.it --kind php-error
```

Per l'object cache di un sito WordPress: `wp plugin activate redis-cache`, poi copia `wp-content/plugins/redis-cache/includes/object-cache.php` in `wp-content/` (dal file manager o con `wp eval`). `wp redis enable` e `wp redis update-dropin` fanno un `FLUSHDB`, che l'ACL nega perché svuoterebbe la cache di tutti i siti.

## Provarlo in locale

```bash
scripts/locale.sh up
```

Crea un `.env` di prova (se non c'è già quello vero), costruisce le immagini e avvia tutto senza Cloudflare: pannello su http://pannello.localhost:8080 (utente `admin@alveare.localhost`, password temporanea con `scripts/locale.sh pass`), siti su `http://<nome>.localhost:8080`, perché i browser risolvono da soli `*.localhost`. `PROFILI="workers php84" scripts/locale.sh up` accende anche worker e PHP 8.4; `down` ferma, `reset` cancella i dati.

## Sviluppo

| Cosa | Comando |
| --- | --- |
| Test Rust (unità e sicurezza) | `cargo test --workspace` |
| Controllo dei tipi della UI | `cd ui && npm ci && npm run check` |
| UI in sviluppo | `cd ui && npm run dev` |
| Immagini | `docker compose build` (serve un `.env`, anche fittizio) |
| CLI | `cargo run -p alveare -- --help` |
| Integrazione completa | `python3 tests/integration.py` (`KEEP=1` lascia lo stack acceso, `E2E=1` aggiunge i test nel browser) |
| Test nel browser su uno stack acceso | `E2E_CONFIG=<cartella del test>/e2e.json npm run e2e` (in `ui/`) |

Il test di integrazione avvia il compose vero (con i profili `workers` e `php84`) e verifica: primo accesso con TOTP, siti WordPress da template, IP reale fino a PHP e nei log, isolamento tra siti, PHP bloccato in uploads, Redis con ACL, database, Adminer con token monouso, upload a blocchi, backup e restore (completo, di un percorso, su nuovo dominio), sospensione, staging con password, redirect, clona, esporta, salva come template e crea da snapshot, import con prefisso diverso, zip ed estrazione, ricerca nei log, override di PHP e Caddy (applicati e rifiutati), blocco IP, cron, aggiornamenti WordPress, WP-CLI, passaggio a PHP 8.4 e ritorno, worker supervisionati con l'utente del sito, chiave di deploy, metriche, riavvio del pannello.

### Struttura

| Percorso | Contenuto |
| --- | --- |
| `crates/core` | Regole condivise: domini e slug, pool FPM e runtime PHP, Caddyfile e controllo degli override, ACL Redis, protocollo dell'agente |
| `crates/php-agent` | Agente nei container PHP: supervisiona FPM, applica i pool, esegue WP-CLI, script e deploy con l'utente del sito; nel container php-workers supervisiona i worker |
| `crates/panel` | Pannello: API, autenticazione (password + TOTP, passkey, inviti), job, file manager, log, database, backup, scheduler, monitoraggio, Cloudflare |
| `crates/cli` | CLI `alveare` sulla stessa API |
| `ui` | SvelteKit + shadcn-svelte, incorporata nel binario del pannello |
| `docker/` | Immagini del pannello e di PHP (8.5, WP-CLI, Adminer) |
| `templates/` | Template dei siti (`wordpress`: ricetta; `laravel`: app da git; i template snapshot salvati dal pannello) |
| `scripts/` | Aggiornamento e ritorno alla versione precedente |

## Stato rispetto alla roadmap

**Fase 1 (MVP), fase 2 (migrazione) e fase 3 (estensioni): fatte**, vedi la roadmap in [docs/spec.md](docs/spec.md).

- **Fase 2**: importa (archivio + dump, prefisso letto dal vecchio wp-config.php) e clona sito, salva come template snapshot e crea da snapshot, restore di un percorso, su nuovo dominio e di prova mensile; zip ed estrazione nel file manager, ricerca nei log ruotati, errori PHP raggruppati, retention dei log per sito; vista aggiornamenti WordPress con aggiornamento in blocco e aggiornamenti notturni secondo la regola del sito; monitoraggio (raggiungibilità, spazio, 5xx, richieste lente) e avvisi su Telegram; cron personalizzati; override di PHP e Caddy con storico, diff e ripristino; budget di memoria; sospensione e protezione dello staging; blocco IP; regole Cloudflare da un modello unico; esporta sito; aggiornamento del pannello; CLI; test Playwright.
- **Fase 3**: passkey, inviti e reset via email, sessioni e token dal profilo; hostname dei tunnel Cloudflare via API; più versioni di PHP e canary; app Laravel con worker supervisionati e deploy da git; metriche Prometheus per Grafana.

Non verificato su questo computer: ARM64 reale (gira la CI su runner ARM), Cloudflare vero (API provate contro un finto Cloudflare), R2 (restic provato su un repository locale), SES (email provate in memoria), passkey in un browser vero (provate con un autenticatore software), deploy di un'app Laravel completa da un repository vero.

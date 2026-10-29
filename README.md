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

I container sono sei e restano sei qualunque sia il numero di siti: `panel`, `php`, `caddy`, `mariadb`, `redis` e i connettori `cloudflared` (fuori dal compose). Tutto ciò che serve a un sito si rigenera dallo stato SQLite del pannello: Caddyfile, pool FPM, utenti Unix, ACL di Redis.

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
2. Accedi con l'email dell'admin e la password temporanea: al primo accesso cambi la password e attivi il TOTP.
3. **Scarica il kit di ripristino** (Account → Kit di ripristino) e conservalo fuori dal server.
4. Crea un sito, poi aggiungi il suo hostname al tunnel verso `http://caddy:80`.

Per l'object cache di un sito WordPress: `wp plugin activate redis-cache`, poi copia `wp-content/plugins/redis-cache/includes/object-cache.php` in `wp-content/` (dal file manager o con `wp eval`). `wp redis enable` e `wp redis update-dropin` fanno un `FLUSHDB`, che l'ACL nega perché svuoterebbe la cache di tutti i siti.

## Sviluppo

| Cosa | Comando |
| --- | --- |
| Test Rust (unità e sicurezza) | `cargo test --workspace` |
| Controllo dei tipi della UI | `cd ui && npm ci && npm run check` |
| UI in sviluppo | `cd ui && npm run dev` |
| Immagini | `docker compose build` (serve un `.env`, anche fittizio) |
| Integrazione completa | `python3 tests/integration.py` (`KEEP=1` lascia lo stack acceso) |

Il test di integrazione avvia il compose vero e verifica: primo accesso con TOTP, creazione di due siti WordPress da template, IP reale fino a PHP e nei log, isolamento tra siti, PHP bloccato in uploads, Redis con ACL, database (export, import, cambio password), Adminer con token monouso, upload a blocchi, backup e restore con restic, eliminazione con backup finale, riavvio del pannello.

### Struttura

| Percorso | Contenuto |
| --- | --- |
| `crates/core` | Regole condivise: domini e slug, pool FPM, Caddyfile, ACL Redis, protocollo dell'agente |
| `crates/php-agent` | Agente nel container PHP: supervisiona FPM, applica i pool, esegue WP-CLI con l'utente del sito |
| `crates/panel` | Pannello: API, autenticazione, job, file manager, log, database, backup, scheduler |
| `ui` | SvelteKit + shadcn-svelte, incorporata nel binario del pannello |
| `docker/` | Immagini del pannello e di PHP (8.5, WP-CLI, Adminer) |
| `templates/` | Template dei siti (`wordpress`: ricetta con core scaricato al momento) |

## Stato rispetto alla roadmap

**Fase 1 (MVP): fatta.** Compose completo; creazione ed eliminazione dei siti da template con rollback; file manager con upload a blocchi ed editor; database con export, import, cambio password e Adminer; log in tempo reale; cron WordPress; backup restic notturni con restore; controllo d'integrità notturno; Redis con un utente per sito; multi-utente con password, TOTP, ruoli per sito, token API e audit log; kit di ripristino; CI su ARM64 e AMD64.

Da fare (fasi 2 e 3, dopo il gate "un sito vero in produzione con backup verificati"): importa e clona sito, template snapshot, override di PHP e Caddy, cron personalizzati, sospensione e protezione dello staging, vista aggiornamenti WordPress, monitoraggio e avvisi su Telegram, blocco IP, esportazione del sito, CLI `alveare`, Laravel con worker, più versioni di PHP, passkey e inviti via email, test Playwright.

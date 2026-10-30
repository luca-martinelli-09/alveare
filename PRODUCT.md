# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users
- **Admin tecnici** (chi installa e mantiene il server): creano siti, gestiscono utenti, runtime PHP, override, Cloudflare, backup. Vogliono densità, velocità e stato del server a colpo d'occhio.
- **Gestori di un sito** (persone di associazioni o clienti, spesso non tecnici): vedono solo i siti assegnati, con ruolo gestore o sola lettura; caricano file, guardano log, ripristinano backup. Serve chiarezza, poco gergo, azioni pericolose ben segnalate.
- Si usa da desktop e da telefono alla pari: anche interventi veri da mobile, non solo controlli.

## Product Purpose
Pannello self-hosted per ospitare molti siti WordPress e PHP su un solo server (ARM64 o AMD64) con un numero fisso di container: Caddy, PHP-FPM, MariaDB, Redis e il pannello in Rust. Successo: creare, mettere in sicurezza, monitorare e ripristinare un sito senza toccare la shell.

## Positioning
Leggero e tutto in Docker, senza un container per sito: un pool FPM e un utente Unix per sito, Caddy comandato via Admin API, tunnel Cloudflare, backup restic con restore di prova automatico. Niente console shell nel pannello per scelta.

## Operating Context
- Sta dietro Cloudflare (tunnel e, facoltativamente, Access). TLS lo fa Cloudflare.
- Operazioni lunghe (backup, restore, clone, import, deploy) girano come job con log in tempo reale.
- Avvisi via Telegram; metriche Prometheus per Grafana; CLI `alveare` per script e WP-CLI.
- Interfaccia in italiano.

## Capabilities and Constraints
- Siti da template (WordPress, PHP, Laravel), file manager, database con Adminer, log, cron, backup e restore (completo, percorso, nuovo dominio), staging, sospensione, clona/esporta/importa, aggiornamenti WordPress notturni, override PHP e Caddy versionati, blocco IP, runtime PHP multipli, worker supervisionati, deploy da git.
- Multi-utente con ruoli per sito, password + TOTP (disattivabile con ALVEARE_TOTP=false), passkey, inviti e reset via email, token API, audit log.
- UI: SvelteKit + shadcn-svelte (stile vega, icone lucide), servita dal binario del pannello.

## Brand Commitments
- Nome: **Alveare**. Progetto open source pubblico: deve presentarsi bene anche a chi lo scopre su GitHub.
- Nessun logo o marchio preesistente.
- Linguaggio visivo: lo standard della categoria (dashboard di hosting), eseguito con cura; scelto il 2026-10-30 al posto delle direzioni più espressive. Riferimenti di livello: Vercel / Railway e la dashboard di Cloudflare.
- Da evitare: tema api/miele come costume, dashboard SaaS generica (gradienti, metriche finte), più clic o più scroll per le azioni frequenti.

## Evidence on Hand
- Specifica completa in docs/spec.md; README con stato per fase.
- Nessun utente esterno, testimonianza o numero d'uso: non inventarne.

## Product Principles
1. Lo stato prima delle azioni: ogni sito dice subito se è sano, sospeso, in staging, con backup recenti.
2. Le azioni pericolose sono chiare e reversibili quando possibile (cestino, backup prima del restore).
3. Comprensibile per chi non è tecnico senza rallentare chi lo è.
4. Funziona davvero da telefono.

## Accessibility & Inclusion
Nessun requisito specifico raccolto oltre a una base WCAG 2.1 AA.

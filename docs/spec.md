 · 

## Obiettivi e requisiti
Un pannello self-hosted, tutto in Docker e compatibile ARM64, per ospitare più siti WordPress e PHP su un solo server, senza un container per sito.
Requisiti
- Gira interamente in container Docker, su ARM64 (Oracle Cloud) e AMD64.

- Numero di container fisso: aggiungere un sito non avvia container nuovi.

- Reverse proxy Caddy, collegabile a uno o più connettori cloudflared.

- Un database MariaDB unico, con un database e un utente dedicati per sito.

- Ogni sito ha la sua cartella root; può essere WordPress o PHP semplice.

- Creazione dei siti da template con placeholder (es. WordPress già configurato).

- File manager web: upload, modifica, rinomina, elimina, zip/unzip.

- Visualizzazione dei log di accesso e degli errori PHP per sito.

- Backup automatici con restore.

- Servizio di gestione scritto in Rust.

- Multi-utente: ogni persona ha il suo accesso e vede solo i siti che le sono assegnati.

Non obiettivi (per ora)
- Hosting email, DNS autoritativo, gestione TLS lato server (la fa Cloudflare).

- Isolamento tra siti forte come un container per sito: si accetta l'isolamento a livello di utente Unix e pool FPM.

- Scalabilità su più server.

- Servizi con stack propri (es. palchi, pretix): restano compose separati, fuori dal pannello.

## Architettura
Cinque container fissi in un unico compose: Caddy, PHP-FPM, MariaDB, Redis e il pannello Rust. I connettori cloudflared restano fuori e raggiungono Caddy sulla rete esterna cloudflared.
Il traffico entra dai tunnel in Caddy, che passa il PHP al pool del sito tramite socket. Il pannello, in evidenza perché è l'unico componente da scrivere, comanda Caddy via Admin API, PHP tramite l'agente e MariaDB direttamente, e manda i backup allo storage esterno.
Container
Immagine
Ruolo
Reti
caddy
caddy:2
Reverse proxy HTTP, file statici, inoltro PHP ai socket FPM
control, cloudflared (non hosting: i siti non lo raggiungono dall'interno)
php
immagine custom basata su php:8.5-fpm
Un pool FPM per sito, WP-CLI incluso
hosting
mariadb
mariadb:12
Un database e un utente per sito
hosting
redis
redis:8
Cache oggetti di WordPress e code di Laravel, un utente ACL per sito
hosting
panel
immagine Rust (multi-arch)
API, UI, template, file manager, log, backup, cron
hosting
Redis: memoria massima fissa con eviction allkeys-lru, così una cache piena non fa crescere il container. Ogni sito ha un utente ACL limitato alle chiavi con prefisso &lt;slug&gt;:; utente, password e prefisso stanno nel .env del sito. Il template WordPress include il plugin di object cache, attivabile sito per sito.
Per le app Laravel si aggiunge un sesto container fisso, php-workers, per i processi long-running (vedi App Laravel e processi long-running).
Caddy usa l'immagine ufficiale, senza moduli aggiuntivi: le protezioni HTTP stanno su Cloudflare (vedi Protezioni HTTP).
Caddy non pubblica porte sull'host: tutto il traffico arriva dai tunnel. L'Admin API di Caddy ascolta su un socket Unix in un volume condiviso con il pannello, non sulla rete.
Struttura delle cartelle (volumi relativi alla cartella del compose):
&lt;/          template dei siti\n  data/\n    sites/]]>&lt;/public/      document root\n    logs/]]>&lt;/              access.log, php-error.log, php-slow.log\n    tmp/]]>&lt;/               upload temporanei e sessioni PHP\n    php/pools/]]>&lt;.conf     pool FPM generati dal pannello\n    php/etc/passwd, group        utenti dei siti, generati dal pannello\n    run/php/]]>&lt;.sock       socket FPM, condivisi con Caddy\n    run/caddy/admin.sock         Admin API di Caddy\n    caddy/                       dati e config di Caddy\n    db/                          dati MariaDB\n    panel/panel.db               SQLite del pannello\n    backups/                     cache restic e dump temporanei]]>Regola chiave: data/sites è montata allo stesso path (/srv/sites) in Caddy, PHP e pannello, altrimenti FPM riceve percorsi che non esistono.
Cloudflare: ogni account Cloudflare ha il suo connettore; tutti puntano a http://caddy:80. Caddy instrada in base all'header Host. Aggiungere un sito = creare il sito nel pannello + aggiungere l'hostname pubblico al tunnel giusto (manuale nell'MVP, via API Cloudflare in seguito).
Ambienti: lo sviluppo e i test girano su una VM Proxmox con lo stesso compose.yml; la produzione sul server Oracle ARM. Le differenze stanno solo nel .env.
Fuso orario: Europe/Rome ovunque, cioè TZ di tutti i container, date.timezone di PHP, time_zone di MariaDB e lo scheduler del pannello. Così orari di cron, log e backup coincidono.

## Esecuzione PHP
Un solo container PHP-FPM, con un pool per sito che gira con un utente Unix dedicato e ascolta su un proprio socket.
Pool tipo (generato dal pannello in data/php/pools/&lt;dominio&gt;.conf):
Utenti: ogni sito ha un UID/GID dedicato (es. da 20000 in su). Il pannello genera passwd e group e li monta in sola lettura nel container PHP, così gli utenti sopravvivono ai riavvii. La cartella del sito appartiene a quell'utente: WordPress può aggiornarsi da solo.
Reload: il pannello scrive il pool, esegue php-fpm -t e, solo se la config è valida, invia un reload graceful (USR2 al master). Se il test fallisce, il pool nuovo viene scartato e il sito resta in errore nel pannello. Un pool sbagliato non deve mai bloccare il reload degli altri siti.
Immagine PHP: una sola immagine multi-arch con le estensioni richieste da WordPress (gd o imagick, intl, zip, mysqli, opcache, exif, redis opzionale), WP-CLI e l'agente del pannello. Nessun client SMTP: le email passano dal mu-plugin SMTP del template.
Versioni di PHP: se servono più versioni, un container per versione (es. php84, php85), mai uno per sito. Il pool vive nel container della versione scelta e Caddy punta al socket giusto.
Limiti accettati: niente cgroup per sito; il consumo è limitato solo da pm.max_children e memory_limit. Un sito sotto attacco può rallentare gli altri: si mitiga con rate limit su Cloudflare.
Budget di memoria: il pannello calcola il massimo teorico di ogni pool (pm.max_children × memory_limit) e la somma su tutti i siti, più worker, MariaDB, Redis e Caddy. La pagina del server mostra il totale rispetto alla RAM. Quando si crea un sito o si alza un limite, il pannello avvisa se il totale supera la RAM, e chiede conferma esplicita oltre il 150%. Non blocca: con pm = ondemand i pool raramente sono tutti al massimo insieme, ma l'avviso deve esserci.

## Configurazioni personalizzate
PHP e Caddy si possono modificare dal pannello, per sito e globalmente, come override sopra i file generati: i file generati non si toccano, perché il pannello li riscrive a ogni modifica.
Livello
Cosa si modifica
Validazione prima di applicare
Chi
PHP del sito
Form per i valori comuni (memory_limit, upload, max_execution_time, pm.max_children, versione PHP) + editor per direttive extra php_value / php_admin_value
php-fpm -t tramite l'agente
Form: tutti; direttive extra: admin
.user.ini del sito
Valori PHP modificabili per cartella, anche dal file manager
Nessuna; PHP lo rilegge ogni 5 minuti
Chi ha accesso ai file
Caddy del sito
Snippet dentro il blocco del sito: header, redirect, cache, blocco di path
POST /adapt dell'Admin API, che valida senza caricare
Admin
PHP globale
conf.d/zz-custom.ini montato nel container
php-fpm -t
Admin
Caddy globale
Opzioni globali e snippet riusabili dai siti
POST /adapt
Admin
Versioni: ogni override è salvato con autore e data; il pannello mostra il diff e ripristina una versione precedente con un clic. Una modifica che non passa la validazione non viene mai applicata.
Direttive rifiutate, perché romperebbero l'isolamento tra siti: in PHP open_basedir, user, group, listen, chroot, disable_functions; in Caddy un root fuori da /srv/sites/&lt;dominio&gt; o un php_fastcgi verso il socket di un altro sito. Il pannello le controlla sul JSON restituito da /adapt.

## Servizio Rust
Un workspace Cargo con due binari: il pannello (API, UI, logica) e un piccolo agente che gira dentro il container PHP. Così il pannello non ha mai accesso al docker.sock.
Crate / componente
Ruolo
panel
Server axum: API REST, UI, scheduler (backup, cron, rotazione log)
php-agent
Gira nel container PHP come root; esegue solo comandi in whitelist ricevuti su un socket Unix condiviso
core
Tipi condivisi, validazione domini e path, protocollo panel ↔ agente
Librerie principali: axum + tokio, sqlx (SQLite per lo stato, MySQL per creare DB e utenti), cap-std per l'accesso sicuro ai file, hyperlocal per l'Admin API di Caddy su socket Unix, croner per le espressioni cron, restic invocato come binario.
Interfaccia: SPA SvelteKit (adapter static) con shadcn-svelte e Tailwind, CodeMirror per l'editor. La build viene incorporata nel binario Rust con rust-embed: un solo artefatto da distribuire, nessun server Node.
Comandi dell'agente (whitelist, nient'altro):
- pool.apply: copia i pool in staging, esegue php-fpm -t, se ok ricarica FPM con USR2, restituisce l'esito.

- wp &lt;dominio&gt; &lt;args&gt;: esegue WP-CLI con l'utente del sito, nella sua root.

- fs.chown &lt;dominio&gt; &lt;path&gt;: riassegna i file all'utente del sito.

- cron.run &lt;dominio&gt;: esegue wp cron event run --due-now.

- fpm.reopen_logs: fa riaprire a FPM i file di log dopo la rotazione.

- cron.exec, artisan, composer: cron personalizzati e comandi Laravel, sempre con l'utente del sito.

- worker.start, worker.stop, worker.restart: processi long-running nel container php-workers.

Gestione di Caddy: il pannello genera il Caddyfile completo dallo stato in SQLite, lo salva in data/caddy/Caddyfile (usato all'avvio) e lo carica a caldo con POST /load e Content-Type: text/caddyfile. Nessuna modifica incrementale: ogni cambio rigenera tutto, quindi lo stato non può divergere.
IP reale dei visitatori: dietro il tunnel la connessione arriva sempre da cloudflared. Con client_ip_headers Caddy ricava l'IP del visitatore da CF-Connecting-IP e lo passa a PHP come REMOTE_ADDR, così plugin di sicurezza, rate limit, commenti e log vedono l'IP giusto. Da verificare con un test nella fase 1.
Il pannello stesso è servito da Caddy su un hostname dedicato, protetto da Cloudflare Access.

## Modello dati
SQLite (data/panel/panel.db) è l'unica fonte di verità: Caddyfile, pool FPM e file utenti si rigenerano sempre da qui.
Tabella
Campi principali
Note
sites
id, slug, dominio principale, tipo (wordpress / php / laravel), template, uid, versione PHP, stato, staging, regola aggiornamenti, creato_il
Lo slug dà nome a cartella, utente Unix e DB
domains
id, site_id, hostname, tipo (primary / alias / redirect)
Redirect www/non-www generati da qui
site_databases
site_id, db_name, db_user, password cifrata
Password cifrata con una chiave nel .env
php_settings
site_id, memory_limit, upload_max, max_children, valori extra
Finiscono nel pool del sito
config_overrides
id, site_id (vuoto = globale), tipo (php / caddy), contenuto, versione, autore, creato_il
Storico completo, ripristino di una versione
cron_jobs
id, site_id, espressione, tipo (wp / artisan / php / http), comando, timeout, attivo
I cron di WordPress non stanno qui: sono automatici
cron_runs
id, cron_id, inizio, fine, exit code, output troncato
Ultime 50 per cron
workers
id, site_id, nome, comando, processi, memoria massima, attivo
Eseguiti in php-workers
backup_policies
site_id, frequenza, retention (giorni / settimane / mesi)
Default globale + override per sito
backup_runs
id, site_id, inizio, fine, esito, snapshot_id restic, dimensione
Storico e restore
jobs
id, tipo, site_id, utente, stato, log, inizio, fine
Operazioni lunghe (crea, clona, importa, restore, deploy)
users
id, email, nome, admin (sì/no), hash password, segreto TOTP cifrato, stato (invitato / attivo / bloccato)
Hash Argon2id
user_sites
user_id, site_id, ruolo (gestore / lettura)
Il ruolo vale per quel sito
sessions
id, user_id, creata_il, ultimo_uso, IP, user agent
Revocabili dal profilo
api_tokens
id, user_id, hash, nome, scadenza, ultimo_uso
Limitati ai siti dell'utente
audit_log
id, user, azione, site_id, dettagli, quando
Chi ha fatto cosa
Slug: derivato dal dominio, solo [a-z0-9_], max 24 caratteri (limite pratico per nomi utente e DB). In caso di collisione si aggiunge un suffisso numerico.

## Flussi principali
Ogni flusso gira come job: ogni passo registra cosa ha creato, e in caso di errore il job annulla i passi già fatti in ordine inverso.
Crea sito
- Valida il dominio e genera slug, UID, nome DB, utente DB e password.

- Crea database e utente su MariaDB, con GRANT solo su quel database.

- Aggiunge l'utente Unix ai file passwd/group.

- Crea sites/&lt;dominio&gt;/public, logs/&lt;dominio&gt;, tmp/&lt;dominio&gt;.

- Copia il template e sostituisce i placeholder (vedi Template).

- Se il template ha un dump SQL: lo importa e lancia wp search-replace tramite l'agente.

- Assegna i file all'utente del sito (fs.chown).

- Scrive il pool e chiama pool.apply.

- Rigenera e carica il Caddyfile.

- Mostra gli hostname da aggiungere al tunnel Cloudflare.

Elimina sito
- Esegue un backup finale (obbligatorio, conservato 30 giorni).

- Rimuove la route da Caddy, poi il pool FPM.

- Sposta la cartella del sito in un cestino per 7 giorni prima della cancellazione definitiva.

- Elimina database e utente DB, poi l'utente Unix.

Clona sito: crea un sito nuovo come sopra, ma copia file e database da un sito esistente invece che da un template, poi wp search-replace dal vecchio al nuovo dominio. Serve per staging e per partire da un sito già fatto.
Importa sito esistente: carica un archivio dei file (o indica una cartella già presente sul server) e un dump SQL. Il pannello aggiorna wp-config.php con le nuove credenziali, importa il dump, esegue search-replace se il dominio cambia. Serve per migrare gli stack attuali (epops-wp, proloco-wp, upupa-wp, vivosport-wp).
Migrazione degli stack attuali: a mano, un sito alla volta, con gli strumenti del pannello.
- Dal vecchio stack: dump del database e archivio zip di wp-content.

- Nel pannello: nuovo sito dal template WordPress base con lo stesso dominio, su un hostname temporaneo per le prove.

- Import del dump dalla pagina Database e caricamento dello zip di wp-content dal file manager, poi estrazione al posto di quello del template.

- Controllo del prefisso delle tabelle: se il vecchio sito non usa wp_, va impostato nel .env del sito prima di aprirlo.

- Verifica sul dominio temporaneo, poi nel tunnel l'hostname pubblico passa dal vecchio container a http://caddy:80.

- Il vecchio stack resta fermo ma non cancellato per qualche settimana, come ritorno indietro.

Sospendi sito: Caddy risponde con una pagina di manutenzione (503 con Retry-After) e il pool FPM viene disattivato; file e database restano intatti. La riattivazione ripristina route e pool.
Protezione dello staging: i siti marcati come staging, e tutti i cloni per default, hanno password con basic_auth di Caddy e l'header X-Robots-Tag: noindex, così motori di ricerca e visitatori non trovano le copie. La protezione si toglie con un clic quando il sito va in produzione.
Esporta sito: genera un archivio tar.gz con i file del sito, il dump del database e un LEGGIMI.txt con versione PHP, estensioni richieste e istruzioni di installazione altrove. Le credenziali del .env vengono tolte e sostituite da segnaposto. L'archivio resta scaricabile per 7 giorni da un link a scadenza, poi viene cancellato; l'esportazione finisce nell'audit log.

## Template
Un template è una cartella in templates/ con un manifest, i file del sito e, facoltativi, un dump SQL e una lista di comandi WP-CLI da eseguire dopo la creazione.
Variabili sempre disponibili: DOMAIN, SITE_URL, SLUG, DB_NAME, DB_USER, DB_PASSWORD, DB_HOST, TABLE_PREFIX, le otto chiavi e salt di WordPress (generate a caso per ogni sito).
Placeholder solo nei file elencati in render. Il core di WordPress contiene template JavaScript con la sintassi {{ }}: una sostituzione su tutto l'albero li romperebbe.
Dump SQL: niente placeholder. WordPress salva molte opzioni come array PHP serializzati, che contengono la lunghezza delle stringhe. Sostituire testo di lunghezza diversa nel dump rompe widget e impostazioni dei temi. Il dump si esporta con il dominio fittizio sql_domain, si importa, poi wp search-replace template.local &lt;dominio&gt; lo corregge rispettando la serializzazione.
Due stili di template:
- Ricetta: core scaricato al momento + comandi post_create (installazione, plugin, impostazioni). Leggero e sempre aggiornato; ideale per i template di base.

- Snapshot: file e dump di un sito già pronto, con contenuti e configurazione del tema. Ideale per siti \"chiavi in mano\".

Credenziali in un .env fuori dalla document root. Il pannello scrive sites/&lt;dominio&gt;/.env (credenziali DB, salt, SMTP, ambiente) accanto a public/, quindi Caddy non lo serve mai. Il wp-config.php del template lo legge con poche righe di parser all'inizio (o con vlucas/phpdotenv, come fa Bedrock) e non contiene segreti: è identico per tutti i siti, e cambiare una password significa solo riscrivere il .env. Lo stesso file serve le app Laravel, che lo usano già nativamente.
Email dei siti: tutte le email WordPress passano da AWS SES tramite il mu-plugin SMTP del template. Le credenziali SMTP di SES stanno nel .env globale del pannello e vengono copiate nel .env di ogni sito; mittente e nome mittente sono per sito. La verifica di ogni nuovo dominio in SES (record DKIM) resta manuale; il pannello ricorda di farla quando crea il sito.
wp-config.php del template include sempre: lettura del .env, $_SERVER['HTTPS'] = 'on' quando X-Forwarded-Proto è https (altrimenti loop di redirect dietro Cloudflare), DISABLE_WP_CRON a true (il cron lo esegue il pannello), FS_METHOD a direct, e le costanti di aggiornamento lette dal .env secondo la regola del sito.
In una fase successiva: salva come template a partire da un sito esistente (copia dei file + dump con search-replace verso template.local).

## App Laravel e processi long-running
Un'app Laravel con scheduler e Horizon si può ospitare: la parte web gira come qualsiasi sito PHP, mentre scheduler, code e Horizon richiedono i worker e Redis, entrambi senza container per sito.
Parte
Come gira
Web
Pool FPM del sito, document root su public/ (template laravel)
Scheduler
Cron personalizzato php artisan schedule:run ogni minuto
Code e Horizon
php artisan horizon (o queue:work) come worker long-running
Redis
Container redis condiviso; un utente ACL per sito, limitato alle chiavi con prefisso &lt;slug&gt;:
Deploy
Job del pannello: aggiorna i file (git pull o upload), composer install --no-dev, artisan migrate --force, cache di config, route e view, horizon:terminate per riavviare i worker
Worker: un container fisso php-workers, dalla stessa immagine PHP, dove l'agente fa da supervisore. Avvia i processi definiti per ogni sito con l'utente del sito, li riavvia se terminano (con attesa crescente), li ferma oltre il limite di memoria e scrive l'output in logs/&lt;dominio&gt;/worker-&lt;nome&gt;.log. Separarlo da FPM evita che un reload dei pool tocchi i worker e viceversa. Vale anche per altri processi persistenti, come Reverb.
Requisiti dell'immagine PHP: estensioni pcntl, posix e redis (Horizon le usa), Composer. I worker girano con la CLI, che non legge il pool: l'agente li avvia con php -d open_basedir=... per mantenere il confine sulla cartella del sito. Horizon avvia i suoi processi con proc_open, quindi per la CLI di quel sito quella funzione resta abilitata.
Asset frontend: la build Vite si fa in CI o in locale e si carica; nell'MVP non c'è Node sul server.
Più avanti: deploy senza interruzione con cartelle releases/ e un link current, e deploy automatico da un webhook Git.

## Gestione database
Ogni sito ha una pagina database nel pannello, così per le operazioni comuni non serve SSH.
- Panoramica: dimensione totale, tabelle con righe e dimensione, ordinate per peso (utile per trovare log o transient che crescono).

- Export: dump completo o di singole tabelle, scaricato compresso.

- Import: caricamento di un dump (anche a blocchi), con backup automatico prima; facoltativo wp search-replace se il dump viene da un altro dominio.

- Credenziali: reset della password dell'utente DB, con aggiornamento automatico del .env del sito.

- Query manuali: Adminer, un singolo file PHP, gira come sito interno del pannello con il suo pool e il suo utente, raggiungibile solo dietro Cloudflare Access. Il pannello apre Adminer già autenticato sul database del sito scelto.

## File manager
Integrato nel pannello, limitato alla cartella del sito: ogni operazione lavora dentro sites/&lt;dominio&gt;/ e non può uscirne.
Operazione
Dettagli
Naviga
Lista con dimensione, data, permessi, proprietario; ordinamento e ricerca per nome
Upload
A blocchi da 8 MB, ripresa in caso di interruzione, più file e cartelle insieme (drag and drop)
Download
File singoli; cartelle come zip generato al volo in streaming
Modifica
Editor CodeMirror con evidenziazione per PHP, JS, CSS, HTML, JSON, ini; limite 2 MB per file
Crea
File e cartelle vuoti
Rinomina / sposta
Anche tra sottocartelle dello stesso sito
Elimina
Va nel cestino del sito (tmp/&lt;dominio&gt;/.trash), svuotato dopo 7 giorni
Estrai / comprimi
Zip e tar.gz, utile per temi e plugin; controllo dei path dentro l'archivio
Permessi
chmod; il proprietario resta sempre l'utente del sito
Perché a blocchi: sui piani Cloudflare Free e Pro una singola richiesta non può superare 100 MB. Il browser spezza il file, il pannello ricompone i pezzi in tmp/&lt;dominio&gt;/ e sposta il file finito al suo posto.
Sicurezza dei path: tutto passa per cap-std, che apre la root del sito come capability e rifiuta .. e symlink che escono dalla cartella. Vale anche per i file dentro gli archivi (zip slip).
Proprietario: il pannello scrive come root, quindi dopo ogni scrittura chiama fs.chown sull'agente. Senza questo passaggio WordPress non riesce più ad aggiornare plugin e core.

## Log
Ogni sito ha tre log in logs/&lt;dominio&gt;/, più il debug.log di WordPress se presente; il pannello li mostra in tempo reale con filtri.
Log
Chi lo scrive
Formato
Rotazione
access.log
Caddy
JSON
Caddy, 10 MB × 5 file
php-error.log
PHP-FPM (pool del sito)
testo
Pannello, 10 MB × 5 file
php-slow.log
PHP-FPM, richieste oltre 5 s con stack trace
testo
Pannello, 5 MB × 3 file
wp-content/debug.log
WordPress, solo con WP_DEBUG_LOG
testo
Pannello, solo lettura e svuotamento
Visualizzazione:
- Tail in tempo reale via Server-Sent Events, con pausa e ripresa.

- Access log: tabella con ora, IP reale (da CF-Connecting-IP), metodo, path, stato, durata, user agent; filtri per stato (2xx, 3xx, 4xx, 5xx), IP e path.

- Errori PHP: raggruppati per messaggio con conteggio, per vedere subito l'errore che si ripete.

- Ricerca nei file ruotati, download del log.

La rotazione dei log PHP la fa il pannello: rinomina il file e chiede a FPM di riaprirli (USR1 tramite l'agente).
Retention: oltre al limite di dimensione, i log vengono eliminati dopo 30 giorni (configurabile per sito, roll_keep_for in Caddy, pulizia del pannello per i log PHP). Gli access log contengono indirizzi IP, quindi dati personali: con siti di enti e associazioni la durata di conservazione deve essere definita e documentata.
Blocca IP dalla vista dei log: su ogni riga c'è l'azione \"Blocca IP\", con durata (1 ora, 24 ore, 7 giorni, sempre) e ambito (solo questo sito o tutti, quest'ultimo solo per admin). Il blocco viene salvato in SQLite e finisce nel Caddyfile generato come regola sul matcher client_ip integrato in Caddy, che risponde 403; il ricaricamento a caldo lo applica in pochi secondi e il pannello lo toglie alla scadenza. Una pagina elenca i blocchi attivi, chi li ha creati e quando scadono. È un blocco sul server: per fermare qualcuno prima del tunnel si usa Cloudflare.

## Backup e restore
Backup per sito con restic: dump del database + cartella del sito, cifrati, incrementali e deduplicati, verso uno storage S3-compatibile esterno al server.
Come funziona un backup
- mariadb-dump --single-transaction del database del sito in backups/&lt;dominio&gt;/db.sql.

- restic backup di sites/&lt;dominio&gt;/ + il dump, con tag site=&lt;slug&gt;.

- restic forget --prune secondo la retention del sito.

- Registra snapshot, dimensione ed esito in backup_runs; in caso di errore lo segnala nel pannello (e in seguito via email o Telegram).

Default proposti: backup ogni notte tra le 3 e le 6 (siti sfalsati, fuori dall'ora del cambio d'ora), retention 7 giornalieri, 4 settimanali, 6 mensili. Un repository restic unico per tutti i siti: la deduplica tra siti WordPress (core e plugin uguali) riduce molto lo spazio.
Storage: Cloudflare R2 come default. Endpoint, bucket, regione e chiavi stanno nel .env del pannello, quindi si passa a qualunque altro storage S3-compatibile cambiando solo la configurazione.
Restore, sempre come job:
- Completo: file + database, con backup automatico dello stato attuale prima di sovrascrivere.

- Solo database o solo file.

- Singolo file o cartella: scelto da una vista dello snapshot, utile quando qualcuno cancella un tema o un'immagine.

- Su un nuovo dominio: ripristina lo snapshot come sito nuovo, per verificare un backup senza toccare quello in produzione.

Da fare dal primo giorno: un restore di prova automatico mensile su un dominio interno, che controlla che il sito risponda. Un backup mai ripristinato non conta.
Anche lo stato del pannello (panel.db, templates/, .env cifrato) va nel repository restic, così il server si ricostruisce da zero.
Kit di ripristino: la password del repository restic e la chiave di cifratura del pannello non stanno nei backup in chiaro, e se si perdono i backup diventano illeggibili. Il pannello genera un kit da scaricare (chiavi, endpoint dello storage, istruzioni per ricostruire il server da zero) da conservare fuori dal server, per esempio nel password manager. Il pannello ricorda di riscaricarlo quando una chiave cambia.

## Cron, aggiornamenti e monitoraggio
Cron: due tipi, pianificati dallo scheduler del pannello ed eseguiti dall'agente con l'utente del sito.
- WordPress: con DISABLE_WP_CRON, cron.run ogni 5 minuti per ogni sito WordPress, sfalsando i siti. Automatico, senza configurazione.

- Personalizzati: per sito, un'espressione cron e un comando, che può essere WP-CLI, php artisan, uno script PHP nella cartella del sito o una chiamata HTTP a un URL. Ogni cron ha un timeout (default 10 minuti), un lock che impedisce esecuzioni sovrapposte, l'output delle ultime 50 esecuzioni e un avviso se fallisce. Dal pannello si mette in pausa, si riattiva o si esegue subito.

Aggiornamenti, su due livelli:
- WordPress (core, plugin, temi): ogni sito ha una regola di aggiornamento, scelta alla creazione e modificabile dopo:
- Automatico: core, plugin e temi si aggiornano da soli, di notte, dopo il backup.

- Solo sicurezza (default): solo le versioni minori del core (patch di sicurezza); plugin e temi restano manuali.

- Manuale: niente si aggiorna da solo; tutto passa dal pannello.

Il pannello applica la regola con le costanti nel .env del sito (WP_AUTO_UPDATE_CORE, AUTOMATIC_UPDATER_DISABLED) e il filtro degli aggiornamenti automatici dei plugin nel mu-plugin del template. Ogni aggiornamento, automatico o manuale, finisce nell'audit log con le versioni prima e dopo. La vista aggiornamenti legge le versioni via WP-CLI e permette l'aggiornamento in blocco, sempre con backup prima.

- Container (PHP, Caddy, MariaDB, pannello): tag con versione major fissata (es. mariadb:12). Notifica delle nuove immagini con Diun, aggiornamento manuale. Niente aggiornamento automatico di MariaDB.

Monitoraggio:
- Spazio su disco per sito e dimensione di ogni database, con soglia di avviso.

- Check di raggiungibilità di ogni sito ogni 5 minuti (stato HTTP e tempo di risposta), dal pannello verso Caddy.

- Errori 5xx e richieste lente per sito, letti dai log.

- Controllo di integrità ogni mattina tra le 7 e le 8, dopo backup e aggiornamenti: wp core verify-checksums e wp plugin verify-checksums --all su ogni sito WordPress, più la ricerca di file .php in wp-content/uploads/. File modificati o estranei rispetto alle versioni ufficiali generano un avviso: è il modo più economico per accorgersi di un sito compromesso.

- Metriche Prometheus di Caddy, esportabili verso Grafana se serve una vista esterna.

- Avvisi: nel pannello e su Telegram, tramite un bot con chat configurabile. Stesso canale per backup falliti, siti irraggiungibili, controllo d'integrità e budget di memoria.

## Utenti e permessi
Il pannello è multi-utente dalla fase 1: ogni chiamata API verifica chi è l'utente e su quali siti può agire, prima di toccare file, database o agente.
Ruolo
Cosa può fare
Admin
Tutto: crea ed elimina siti, gestisce utenti, template, override grezzi, configurazione globale, backup di tutti i siti
Gestore
Sui siti assegnati: file manager, log, database e Adminer, cron, worker, aggiornamenti WordPress, backup e restore, impostazioni PHP dal form, sospensione
Sola lettura
Sui siti assegnati: log, stato, backup e monitoraggio, senza modifiche
Il ruolo si assegna per sito: la stessa persona può essere gestore di un sito e sola lettura su un altro. Solo gli admin creano o eliminano siti e vedono le impostazioni globali.
Accesso
- Email e password (hash Argon2id) con secondo fattore TOTP obbligatorio; passkey in fase 3.

- Sessione in cookie HttpOnly e SameSite=Strict, scadenza dopo 12 ore di inattività; protezione CSRF sulle richieste che modificano.

- Blocco temporaneo dopo 5 tentativi falliti per account e per IP.

- Token API personali per script, con scadenza e limitati ai siti dell'utente.

- Credenziali nuove (token API, passkey) e cambio email solo da una sessione del browser e con la password attuale. Cambio o reset della password revocano sessioni e token; password temporanea e azzeramento del TOTP decisi dall'admin tolgono anche le passkey.

- Nell'MVP l'admin crea l'utente con una password temporanea da cambiare al primo accesso; in fase 3 inviti via email con link a scadenza (serve l'SMTP del pannello).

- Cloudflare Access resta come livello esterno facoltativo davanti al pannello.

Tracciabilità: ogni azione che modifica qualcosa finisce nell'audit log con utente, sito e dettagli; il gestore vede quello dei suoi siti, l'admin tutto. Anche i comandi verso l'agente sono vincolati al sito: un gestore non può far eseguire WP-CLI o leggere file di un sito non suo.

## Sicurezza
Il confine tra i siti è l'utente Unix del pool più open_basedir; tutto il resto riduce cosa può fare un sito compromesso o un pannello esposto.
Rischio
Contromisura
Un sito legge i file di un altro
Utente Unix per sito, cartelle 0750, open_basedir, tmp e sessioni separati
Un sito legge script compilati di un altro dalla cache
Opcache è condivisa tra i pool dello stesso master: opcache.validate_root=1 e opcache.validate_permission=1 nella config globale
Un sito legge il database di un altro
Utente DB per sito con GRANT solo sul suo database; root MariaDB usato solo dal pannello
Un sito legge le chiavi Redis di un altro
Utente ACL Redis per sito, limitato al prefisso &lt;slug&gt;:
Override di configurazione usati per uscire dalla cartella
Direttive pericolose rifiutate (vedi Configurazioni personalizzate); override grezzi solo per admin
Accesso al pannello
Login con password e TOTP, sessioni con scadenza, blocco dopo tentativi falliti, permessi per sito verificati su ogni API; Cloudflare Access facoltativo davanti
Pannello compromesso
Nessun docker.sock; l'agente accetta solo comandi in whitelist con argomenti validati
Path traversal nel file manager
cap-std, controllo symlink, controllo dei path negli archivi
Brute force su WordPress
Rate limit su wp-login.php e xmlrpc.php (regole di rate limiting di Cloudflare), basato sull'IP reale
Credenziali in chiaro
.env dei siti fuori dalla document root e leggibile solo dall'utente del sito; password DB cifrate in SQLite
Upload di PHP malevolo in uploads
Caddy blocca l'esecuzione di .php in wp-content/uploads/; il controllo notturno li segnala
Funzioni PHP pericolose
disable_functions per pool (exec, shell_exec, system, passthru, proc_open, popen), sovrascrivibile per sito
WP-CLI gira sempre con l'utente del sito, mai come root, così un plugin malevolo non ottiene privilegi durante un comando del pannello.

### Protezioni HTTP
Le protezioni HTTP stanno tutte su Cloudflare, prima che il traffico entri nel tunnel; Caddy resta l'immagine ufficiale, senza moduli da ricompilare e mantenere. Caddy non ha fail2ban né rate limit nel binario standard (stackharbor, forum Caddy), e fail2ban classico dietro il tunnel non servirebbe comunque: tutto il traffico arriva da cloudflared.
Protezione
Dove
Regole WAF gestite e regole personalizzate (blocco di xmlrpc.php, user agent di scanner noti)
Cloudflare, per ogni dominio
Rate limit su wp-login.php e wp-admin/admin-ajax.php
Cloudflare, regole di rate limiting
Bot e scansioni automatiche
Cloudflare, protezione dai bot
Blocchi manuali di IP decisi dal pannello
Caddy, matcher client_ip nel Caddyfile generato
Scelte
- Niente moduli aggiuntivi in Caddy: nessuna build custom, si aggiorna come le altre immagini ufficiali.

- Cloudflare non si può aggirare: il server non espone porte, quindi tutto il traffico passa dal tunnel e dalle regole di Cloudflare.

- Regole da configurare per ogni dominio, nei limiti del piano Cloudflare di quel dominio. In fase 3, insieme agli hostname dei tunnel, il pannello potrà applicarle via API da un modello unico.

- Valutati e scartati: il modulo di rate limit per Caddy, CrowdSec con il suo WAF e Coraza con le regole OWASP (guida CrowdSec, guida Coraza). Aggiungono un'immagine Caddy custom e lavoro di manutenzione per una protezione che Cloudflare offre già al bordo.

## Aggiornamenti e test del pannello
Ogni aggiornamento dello stack ha un backup prima e un modo per tornare indietro; ogni modifica al codice passa da un test che crea ed elimina un sito vero.
Aggiornare il pannello
- Backup di panel.db (copia locale + snapshot restic).

- Nuova immagine del pannello con tag di versione, mai latest.

- All'avvio il pannello applica le migrazioni dello schema SQLite (incluse nel binario con sqlx::migrate!) e rigenera Caddyfile, pool e file utenti dallo stato.

- Se qualcosa non va: immagine precedente + ripristino di panel.db. Le migrazioni sono solo additive tra versioni minori, così il rollback non perde dati.

Aggiornare l'immagine PHP: si avvia un container php-canary con la nuova immagine, ci si sposta un sito di prova (Caddy punta al suo socket) e si controllano log e checksum. Se va bene, la nuova immagine sostituisce quella di tutti i siti; se no, il sito torna sul container attuale. Stessa idea per una nuova versione di PHP.
Caddy e MariaDB: aggiornamenti manuali dopo la notifica di Diun, con backup completo prima; per MariaDB solo versioni minori senza test separato.
Test
Livello
Cosa copre
Dove gira
Unità
Validazione di domini, slug e path; generazione di Caddyfile e pool; rifiuto delle direttive pericolose; parsing dei template
cargo test, a ogni commit
Sicurezza
Path traversal, symlink e zip slip nel file manager; override che tentano di uscire dalla cartella; comandi non in whitelist verso l'agente
cargo test, a ogni commit
Integrazione
Compose completo: crea un sito WordPress da template, risponde 200 con l'IP reale, clona, backup, restore su nuovo dominio, sospendi, elimina, verifica che non restino DB, utente, pool e route
CI su runner ARM64 e AMD64, a ogni pull request
Interfaccia
Flussi principali nel browser (crea sito, carica un file, legge un log)
Playwright, prima di ogni release

## API REST
La UI usa la stessa API, quindi tutto ciò che si fa dal pannello si può anche automatizzare (script, CLI, agenti). Le operazioni lunghe rispondono 202 con un id di job.
Metodo e path
Azione
GET /api/sites
Elenco siti con stato, dominio, spazio usato
POST /api/sites
Crea sito da template (job)
GET /api/sites/{id}
Dettaglio sito
PATCH /api/sites/{id}
Modifica impostazioni PHP, versione, stato
DELETE /api/sites/{id}
Elimina sito (job, con backup finale)
POST /api/sites/{id}/clone
Clona su un nuovo dominio (job)
POST /api/sites/import
Importa sito esistente (job)
GET / POST / DELETE /api/sites/{id}/domains
Gestione alias e redirect
GET / PUT /api/sites/{id}/config/{tipo}
Override PHP o Caddy, con validazione; GET .../history per le versioni
GET / POST /api/sites/{id}/cron
Elenco e creazione dei cron personalizzati; POST .../{cid}/run per eseguire subito
GET / POST /api/sites/{id}/workers
Worker long-running; POST .../{wid}/restart
POST /api/sites/{id}/deploy
Deploy di un'app Laravel (job)
GET /api/sites/{id}/files?path=
Lista cartella
GET / PUT /api/sites/{id}/files/content?path=
Leggi / salva file
POST /api/sites/{id}/files/uploads
Avvia upload a blocchi; PUT .../uploads/{uid}/{n} per ogni blocco
POST /api/sites/{id}/files/ops
Rinomina, sposta, elimina, chmod, estrai, comprimi
GET /api/sites/{id}/logs/{tipo}/stream
Tail in tempo reale (SSE)
GET /api/sites/{id}/logs/{tipo}?q=&amp;status=
Ricerca nei log
GET / POST /api/sites/{id}/backups
Elenco snapshot / backup immediato
POST /api/sites/{id}/backups/{snap}/restore
Restore completo, parziale o su nuovo dominio (job)
GET /api/sites/{id}/wordpress
Versioni di core, plugin e temi
POST /api/sites/{id}/wordpress/update
Aggiornamento (job, con backup prima)
GET / PUT /api/config/{tipo}
Override globali di PHP o Caddy (solo admin)
GET /api/templates
Elenco template con variabili richieste
GET /api/jobs/{id}
Stato e log di un job
Autenticazione: la UI usa la sessione in cookie, gli script un token API personale (Authorization: Bearer). Ogni endpoint /api/sites/{id}/... verifica il ruolo dell'utente su quel sito. Endpoint aggiuntivi: /api/auth/* (login, TOTP, logout, reset password), /api/users e /api/users/{id}/sites (solo admin), /api/me/tokens e /api/me/sessions.
CLI: un binario Rust (alveare) nello stesso workspace, che usa la stessa API con un token personale e quindi ha esattamente i permessi dell'utente. Comandi principali: sites list, sites create --template, sites clone, sites export, backup run, backup restore, logs tail, wp &lt;sito&gt; -- &lt;comando&gt;, cron run, ip block. Output leggibile o JSON (--json) per gli script. Gira su Linux, macOS e Windows, anche fuori dal server.

## Roadmap per fasi
- **Fase 1 · MVP**: compose con Caddy, PHP-FPM, MariaDB, Redis e pannello; immagine PHP multi-arch con agente e WP-CLI. Crea ed elimina sito da template ricetta. File manager base e gestione database (upload a blocchi, editor, dump, Adminer). Log in tempo reale, cron WordPress, backup restic notturni con restore completo. Hardening (IP reale, opcache isolato, checksum notturni, kit di ripristino, test in CI). Multi-utente (password e TOTP, ruoli per sito, token API, audit log).
- Gate 1 · primo sito reale in produzione: un sito nuovo servito dietro il tunnel, backup e restore di prova riusciti.
- **Fase 2 · Migrazione**: importa e clona sito, template snapshot, restore parziali e su nuovo dominio. Zip ed estrazione nel file manager, ricerca nei log, errori PHP raggruppati. Vista aggiornamenti WordPress con update in blocco, monitoraggio e avvisi. Cron personalizzati, override di PHP e Caddy, rate limit, restore di prova mensile. Sospensione siti, protezione dello staging, retention dei log, aggiornamento del pannello. Regole di aggiornamento, budget di memoria, blocco IP, regole Cloudflare, esporta sito, CLI.
- Gate 2 · stack attuali migrati: epops, proloco, upupa e vivosport sul pannello; vecchi compose spenti.
- **Fase 3 · Estensioni**: login con passkey, inviti via email, gestione delle sessioni dal profilo. Hostname dei tunnel Cloudflare gestiti via API, salva sito come template. Più versioni di PHP, app Laravel con worker e deploy, metriche verso Grafana.

Si passa alla fase successiva solo superato il gate: la fase 2 parte quando un sito vero gira sul pannello con backup verificati, la fase 3 quando i vecchi stack sono spenti.

## Questioni aperte
- Utenti: multi-utente dalla fase 1, con ruoli per sito (vedi Utenti e permessi).

- Dove gira: test su una VM Proxmox, produzione sul server Oracle ARM.

- Storage dei backup: Cloudflare R2, sostituibile con qualunque S3-compatibile cambiando la configurazione.

- Canale degli avvisi: Telegram.

- Email dei siti: AWS SES.

- Versione PHP di default: 8.5.

- Interfaccia: SvelteKit con shadcn-svelte.

- Tunnel Cloudflare: hostname aggiunti a mano; automazione via API in fase 3.

- Servizi non WordPress (palchi, pretix): restano compose separati, fuori dal pannello.

- Cache oggetti: Redis condiviso già nell'MVP, con un utente per sito.

- Protezioni HTTP: solo Cloudflare; Caddy con immagine ufficiale.

- Esposizione dei siti: solo tramite tunnel per ora; proxy Cloudflare o accesso diretto con HTTPS di Caddy rimandati.

- Nome del progetto: Alveare; binario e CLI alveare.

- Migrazione degli stack attuali: a mano, sito per sito, con import del database e upload di wp-content (vedi Flussi principali).


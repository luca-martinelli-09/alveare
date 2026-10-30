---
version: 1
slug: "ui-src-routes-layout-svelte"
primary_target: "ui/src/routes/+layout.svelte"
related_targets: ["ui/src/routes/+page.svelte"]
---

# Pannello Alveare (UI)

Scope: tutta la UI autenticata del pannello (shell, panoramica, sito, template, utenti, server, account) e le pagine di accesso. Modo: Operate.

Pubblico e compito: admin tecnici e gestori di un sito, da desktop e telefono alla pari. Prima cosa all'apertura: lo stato di tutti i siti. Da non perdere: velocità delle azioni frequenti. Da evitare: tema api come costume, SaaS generica, più clic.

## Direction contract

THESIS: lo standard della categoria (dashboard di hosting) eseguito al livello di Vercel/Railway e della dashboard Cloudflare; rifiuta la pagina shadcn di default con barra in alto e tabelle nude.

OWN-WORLD: neutri freddi quasi monocromi, bianco con barra laterale grigio chiarissimo e filetti sottili; Geist Sans e Geist Mono per dati e misure; un solo colore di marca (ambra) solo nel logo; stati con vocabolario fisso: verde online, ambra attenzione, rosso giù o fallito, blu in corso. Radius piccolo, ombre quasi assenti.

STORY: chi entra vede subito quali siti stanno bene e quali no, apre il sito con un clic dalla barra laterale o dalla tabella, lavora nelle sezioni del sito senza perdere il contesto.

FIRST VIEWPORT: barra laterale fissa a sinistra (logo, Panoramica, elenco siti con pallino di stato, Template/Utenti/Server per gli admin, account in fondo); a destra intestazione "Panoramica" con Nuovo sito; striscia di stato su una riga; avvisi aperti come righe compatte; tabella dei siti con risposta, ultimo backup, aggiornamenti, spazio, traffico, problemi in testa. Su telefono la barra diventa un foglio dal pulsante menu.

FORM: canon (uscita standard), seed 8b61f176; riferimenti scelti: Vercel/Railway, Cloudflare dashboard.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

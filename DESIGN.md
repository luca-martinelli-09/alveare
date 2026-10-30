---
name: Alveare
description: Pannello di hosting self-hosted per siti WordPress e PHP; lo stato prima delle azioni.
colors:
  background: "oklch(1 0 0)"
  foreground: "oklch(0.216 0.006 56)"
  primary: "oklch(0.77 0.16 70)"
  primary-foreground: "oklch(0.216 0.006 56)"
  secondary: "oklch(0.97 0.025 85)"
  muted: "oklch(0.977 0.008 80)"
  muted-foreground: "oklch(0.52 0.014 58)"
  accent: "oklch(0.96 0.035 88)"
  border: "oklch(0.93 0.014 80)"
  input: "oklch(0.89 0.018 78)"
  ring: "oklch(0.77 0.16 70)"
  sidebar: "oklch(0.988 0.012 85)"
  sidebar-accent: "oklch(0.96 0.035 88)"
  honey-1: "oklch(0.87 0.15 92)"
  honey-2: "oklch(0.77 0.16 70)"
  honey-3: "oklch(0.7 0.19 48)"
  honey-soft-1: "oklch(0.92 0.11 95)"
  honey-soft-2: "oklch(0.985 0.025 90)"
  honey-wash: "oklch(0.97 0.04 88)"
  brand: "oklch(0.79 0.16 72)"
  success: "oklch(0.6 0.15 152)"
  success-soft: "oklch(0.96 0.03 155)"
  success-text: "oklch(0.45 0.12 152)"
  warning: "oklch(0.65 0.21 42)"
  warning-soft: "oklch(0.95 0.04 65)"
  warning-text: "oklch(0.47 0.15 40)"
  destructive: "oklch(0.57 0.21 25)"
  destructive-soft: "oklch(0.95 0.025 20)"
  info: "oklch(0.58 0.16 255)"
typography:
  headline:
    fontFamily: "Geist Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: "2rem"
    letterSpacing: "-0.025em"
  title:
    fontFamily: "Geist Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 600
    lineHeight: "1.75rem"
    fontFeature: "\"tnum\""
  section:
    fontFamily: "Geist Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: "1.25rem"
  body:
    fontFamily: "Geist Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: "1.25rem"
  label:
    fontFamily: "Geist Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: "1rem"
  mono:
    fontFamily: "Geist Mono Variable, ui-monospace, monospace"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: "1rem"
rounded:
  md: "0.4rem"
  lg: "0.5rem"
  xl: "0.7rem"
  pill: "1.3rem"
  full: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  2xl: "32px"
components:
  button-primary:
    backgroundColor: "{colors.honey-2}"
    textColor: "{colors.primary-foreground}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "36px"
  button-outline:
    backgroundColor: "{colors.background}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "36px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "36px"
  button-ghost-hover:
    backgroundColor: "{colors.muted}"
  button-destructive:
    textColor: "{colors.destructive}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "36px"
  input:
    backgroundColor: "transparent"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "4px 10px"
    height: "36px"
  badge-secondary:
    backgroundColor: "{colors.secondary}"
    textColor: "{colors.foreground}"
    typography: "{typography.label}"
    rounded: "{rounded.pill}"
    padding: "2px 8px"
    height: "20px"
  sidebar-item:
    textColor: "{colors.foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "8px"
  sidebar-item-hover:
    backgroundColor: "{colors.sidebar-accent}"
  sidebar-item-active:
    backgroundColor: "{colors.honey-soft-1}"
  status-dot:
    rounded: "{rounded.full}"
    size: "8px"
  status-pill-ok:
    backgroundColor: "{colors.success-soft}"
    textColor: "{colors.success-text}"
    typography: "{typography.label}"
    rounded: "{rounded.full}"
    padding: "2px 8px 2px 6px"
  status-pill-warn:
    backgroundColor: "{colors.warning-soft}"
    textColor: "{colors.warning-text}"
    typography: "{typography.label}"
    rounded: "{rounded.full}"
    padding: "2px 8px 2px 6px"
  status-pill-down:
    backgroundColor: "{colors.destructive-soft}"
    textColor: "{colors.destructive}"
    typography: "{typography.label}"
    rounded: "{rounded.full}"
    padding: "2px 8px 2px 6px"
  site-avatar:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    size: "28px"
  icon-tile:
    backgroundColor: "{colors.honey-soft-1}"
    rounded: "{rounded.md}"
    size: "24px"
  usage-bar:
    backgroundColor: "{colors.muted}"
    rounded: "{rounded.full}"
    height: "6px"
  row-list:
    backgroundColor: "{colors.background}"
    rounded: "{rounded.lg}"
    padding: "12px 16px"
  setting-row:
    backgroundColor: "{colors.background}"
    rounded: "{rounded.lg}"
    padding: "14px 16px"
  stat-strip:
    typography: "{typography.title}"
    rounded: "{rounded.lg}"
    padding: "12px 16px"
---

# Design System: Alveare

## Overview

**Creative North Star: "La sala macchine in ordine"**

Alveare è un pannello operativo: chi lo apre vuole sapere in un colpo d'occhio quali siti stanno bene e quali no, poi agire con un clic. Il sistema esegue lo standard della categoria (dashboard di hosting) al livello di Vercel, Railway e della dashboard Cloudflare: struttura sobria, filetti sottili al posto delle ombre, Geist Sans per il testo e Geist Mono per misure e identificatori. Su questa struttura si posa la palette miele (scelta esplicita del 2026-10-30): neutri caldi color pietra, un gradiente giallo → ambra → arancio sulle azioni primarie, la voce attiva in un miele tenue, una velatura calda in cima alla pagina.

La densità è da strumento: corpo a 14px, righe compatte, tabella su desktop e lista a una riga su telefono, con lo stesso ordine (prima i siti con problemi). Il miele dice "marchio e azione"; lo stato ha un vocabolario a parte, con l'attenzione in arancio scuro ben distinto dal miele, e si legge sempre anche senza colore (icona più parola nella pillola, nome accessibile sul pallino). Tema chiaro e scuro sono pari, scelti dall'utente (Come il sistema, Chiaro, Scuro).

Rifiuti confermati in PRODUCT.md: il tema api come illustrazione o decorazione (sta nei colori e nel marchio), la dashboard SaaS generica (metriche finte, banner promozionali), i clic o lo scroll in più per le azioni frequenti.

**Key Characteristics:**
- Neutri caldi pietra (tinta 56-88) al posto dei grigi freddi.
- Gradiente miele confinato a pochi posti fissi: bottone primario, voce attiva, velatura di pagina, area del grafico, barra dello spazio.
- Stati a significato fisso decisi da una sola regola in codice; attenzione in arancio scuro, separato dal miele.
- Stato mostrato con pallino o con pillola icona + parola, mai solo col colore.
- Profondità con filetti di 1px e card bianche sulla velatura, quasi senza ombre.
- Geist Sans e Geist Mono, cifre tabulari nei dati; base shadcn-svelte stile vega, icone lucide.

## Colors

Neutri caldi e quieti, un miele saturo che segna azioni e selezione, e quattro colori di stato che non si confondono con il miele.

### Primary
- **Miele Ambra** (primary, honey-2, oklch(0.77 0.16 70)): colore primario e anello di focus (ring); linea attiva delle schede, selezione del testo (al 30%), centro del gradiente miele. In scuro sale a oklch(0.8 0.15 75).
- **Miele Chiaro** (honey-1, oklch(0.87 0.15 92)): inizio giallo del gradiente miele.
- **Miele Bruciato** (honey-3, oklch(0.7 0.19 48)): coda arancio del gradiente, ombra tinta del bottone primario, linea del grafico di risposta.
- **Inchiostro Pietra** (primary-foreground = foreground, oklch(0.216 0.006 56)): il testo sopra il miele è scuro, non bianco (11.8:1 su honey-1, 6.1:1 su honey-3).

### Secondary
- **Polline** (honey-soft-1, oklch(0.92 0.11 95)) e **Panna** (honey-soft-2, oklch(0.985 0.025 90)): il gradiente tenue `bg-honey-soft` della voce attiva della barra laterale e delle tessere icona. In scuro diventano oklch(0.36 0.07 72) e oklch(0.22 0.02 70).
- **Velatura** (honey-wash, oklch(0.97 0.04 88)): `bg-honey-wash`, sfuma in trasparente entro 320px dall'alto dell'area contenuti.

### Neutral
- **Bianco Pieno** (background, card): fondo di card, liste, tabelle e popover; in scuro oklch(0.17 0.006 56), card oklch(0.2 0.007 56).
- **Cera** (sidebar, oklch(0.988 0.012 85)): fondo della barra laterale; in scuro scende sotto il fondo (oklch(0.15 0.006 56)).
- **Crema Miele** (accent, sidebar-accent, oklch(0.96 0.035 88)): hover della barra laterale, fondo dell'avatar del sito.
- **Crema** (secondary, oklch(0.97 0.025 85)): badge secondari, bottone secondario.
- **Nebbia Calda** (muted, oklch(0.977 0.008 80)): hover di righe e bottoni ghost, binario della barra dello spazio, tessere icona dell'attività, fondo delle iniziali utente.
- **Grigio Pietra** (muted-foreground, oklch(0.52 0.014 58)): metadati, etichette di colonna, date relative, descrizioni.
- **Filetto** (border, oklch(0.93 0.014 80)) e **Filetto Campo** (input, oklch(0.89 0.018 78)): bordi e divisori; in scuro bianco al 9% e 14%.

### Stati
- **Verde Online** (success, soft, text): pallino del sito che risponde; pillola su success-soft con testo success-text.
- **Arancio Attenzione** (warning, oklch(0.65 0.21 42)): "Da guardare" (backup vecchio o fallito, errori 5xx); barra dello spazio oltre il 75%. Pillola su warning-soft con warning-text (oklch(0.47 0.15 40)).
- **Rosso Giù** (destructive, destructive-soft): sito che non risponde o in errore, valori falliti, bottoni distruttivi tinti.
- **Blu In Corso** (info): pallino pulsante dei siti in creazione o eliminazione.
- Spento: grigio (muted-foreground al 50%). In attesa del primo controllo: anello vuoto.

### Brand
- **Ambra Cella** (brand, oklch(0.79 0.16 72), uguale nei due temi): la cella in cima al marchio (`fill-brand`) e la favicon. Il token resta nel marchio; il miele dell'interfaccia passa da primary e honey-*.

### Named Rules
**The Miele sulle Azioni Rule.** Il gradiente miele (`bg-honey`, 135°, honey-1 → honey-2 al 55% → honey-3) va sul bottone primario e sul riempimento della barra dello spazio; la sua versione tenue (`bg-honey-soft`) sulla voce attiva e sulle tessere icona; la velatura (`bg-honey-wash`) solo in cima all'area contenuti; l'area del grafico di risposta sfuma honey-2 dal 35% a 0. Altrove il miele non compare: niente gradienti su testo, bordi, card intere o fondi di sezione. Icone nelle tessere e linea del grafico usano l'inchiostro miele (`honey-ink`, oklch(0.52 0.11 70) in chiaro, oklch(0.85 0.12 85) in scuro), che regge 4.5:1 e non si confonde con l'arancio di stato; il segmento del database nella barra dello spazio è honey-1.

**The Vocabolario di Stato Rule.** Verde online, arancio scuro attenzione, rosso giù o fallito, blu in corso, grigio spento, anello vuoto in attesa. Il miele non è mai uno stato e l'arancio di stato non è mai decorazione. Lo stato si calcola in un solo punto (`ui/src/lib/health.ts`) e ogni superficie (barra laterale, palette ⌘K, panoramica, sito) lo legge da lì.

**The Stato Leggibile Rule.** Come testo, gli stati usano le varianti -text (success-text, warning-text, destructive-text, info-text), mai warning, success o info pieni: quelli servono per pallini, barre e icone (warning su bianco è 3.5:1). Il testo di una pillola deve reggere 4.5:1 sul proprio fondo soft.

## Typography

**Body Font:** Geist Variable (con ui-sans-serif, system-ui)
**Label/Mono Font:** Geist Mono Variable (con ui-monospace)

**Character:** Un sans geometrico e neutro da strumento tecnico, con il suo gemello mono per tutto ciò che si misura o si copia. Nessun font display: la gerarchia la fanno peso e dimensione, non il carattere.

### Hierarchy
- **Headline** (600, 1.5rem/2rem, tracking -0.025em): titolo di pagina nell'intestazione (Panoramica, nome host del sito, Template). Uno per pagina.
- **Title** (600, 1.25rem/1.75rem, cifre tabulari): valori della striscia di stato.
- **Section** (500, 0.875rem/1.25rem): titoli di sezione ("Avvisi aperti", "Attività recente", "Siti"), nomi di sito nelle righe, etichette delle impostazioni, voce attiva della barra laterale.
- **Body** (400, 0.875rem/1.25rem): testo corrente, celle di tabella, spiegazioni delle impostazioni. Gli input salgono a 1rem sotto 768px per evitare lo zoom su telefono.
- **Label** (400, 0.75rem/1rem): etichette della striscia, date relative, sottorighe; in peso 500 nelle pillole di stato e nei badge.
- **Mono** (400, 0.75rem/1rem): tempi di risposta ("200 · 84 ms"), assi del grafico (11px), tasto ⌘K, identificatori e percorsi.

### Named Rules
**The Cifre in Colonna Rule.** Celle di tabella, intestazioni, elementi `time` e la classe `.tabular` usano cifre tabulari, impostate globalmente: i numeri si allineano senza lavoro per pagina.

**The Mono per le Misure Rule.** Geist Mono solo per misure, codici e identificatori, mai per titoli o etichette.

## Layout

Barra laterale fissa a sinistra (marchio, ricerca ⌘K, Panoramica, elenco siti con pallino ordinati per salute, gruppo Amministrazione per gli admin, account in fondo) e colonna di contenuto a destra, centrata con larghezza massima 72rem (max-w-6xl), su fondo con velatura miele in cima. Margini del contenuto 16px su telefono e 32px da 768px; ritmo verticale tra i blocchi di 24px; dentro una sezione titolo e contenuto distano 8px; righe e celle della striscia hanno 12px verticali e 16px orizzontali (14px nelle righe di impostazione, 10px nelle righe di attività).

Panoramica: intestazione con azioni, striscia di stato, poi avvisi aperti e attività recente affiancati da 1024px (senza avvisi la colonna sparisce su schermi stretti), poi i siti. Nella scheda del sito, il grafico di risposta occupa due terzi e "In breve" un terzo da 1024px.

Sotto 768px la barra laterale diventa un foglio aperto dal pulsante menu in un'intestazione appiccicosa alta 48px (fondo al 90% con sfocatura), e si chiude a ogni navigazione. Sempre sotto 768px la tabella dei siti lascia il posto a una lista a una riga per sito con avatar (pallino di stato sull'angolo), nome e primo problema; la striscia di stato passa da 4 a 2 colonne sotto 640px. Le righe di impostazione mandano il controllo sotto l'etichetta quando lo spazio scende sotto 18rem. Le schede del sito scorrono in orizzontale senza barra visibile. Il filtro dei siti nella barra laterale compare solo oltre 8 siti.

Le pagine di accesso (login, invito, reset) usano una cornice a parte: fondo muted al 40%, marchio con nome sopra, modulo largo al massimo 24rem al centro.

## Elevation & Depth

Il sistema è piatto: la profondità si legge da filetti di 1px e da scarti tonali (card bianche sulla velatura miele, barra laterale color cera, hover in crema). I contenitori dell'app (striscia, liste, tabella, sezioni di impostazioni, card del monitor) hanno fondo card, bordo e nessuna ombra. Unica ombra nuova: il bottone primario poggia su un'ombra tinta di miele bruciato. Il resto arriva dalle primitive vega: ombra minima su campi e bottoni outline, ombre medie solo su menu, popover e dialoghi.

### Shadow Vocabulary
- **Goccia** (`box-shadow: 0 1px 2px color-mix(in oklch, var(--honey-3) 35%, transparent)`): bottone primario.
- **Appoggio** (`box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05)`, shadow-xs): campi, bottoni outline.
- **Sospensione** (shadow-md / shadow-lg di Tailwind): menu a tendina, select, popover.

### Named Rules
**The Filetto Rule.** Un contenitore dell'app si separa con un bordo di 1px nel colore border e i divisori tra righe sono filetti, non ombre. L'ombra resta agli elementi che galleggiano e al bottone primario.

## Shapes

Raggio piccolo e coerente, derivato da una sola variabile (0.5rem). I contenitori di pagina usano 8px; bottoni, campi, voci di menu, avatar del sito e tessere icona 6.4px; card e dialoghi 11.2px; badge e pillole di stato sono pillole piene; pallino di stato e barra dello spazio sono arrotondati al massimo. Il marchio è un quadrato arrotondato (raggio 7 su 32) in inchiostro pietra con sei celle esagonali (punta in alto, angoli arrotondati) a piramide 1-2-3 come una "A", la cella in cima in ambra. Gli esagoni restano nel marchio.

## Components

### Buttons
Compatti, altezza 36px, testo 14px medio, con icona lucide a sinistra quando l'azione crea qualcosa.
- **Shape:** angoli lievemente arrotondati (6.4px).
- **Primary:** gradiente `bg-honey` con testo inchiostro pietra e ombra Goccia; hover a luminosità 105%. Per l'azione principale della vista (Nuovo sito, Crea sito).
- **Outline:** fondo pagina, bordo, ombra minima; hover su nebbia.
- **Secondary:** fondo crema, hover scurito del 5% verso il testo.
- **Ghost:** trasparente, hover su nebbia; per azioni di riga e icone.
- **Destructive:** fondo rosso al 10% con testo rosso, non un blocco pieno.
- **Press / Focus:** alla pressione scende di 1px; focus con anello di 3px in ambra al 50%.

### Badges
- **Style:** pillola alta 20px, testo 12px medio. Secondary (crema) per conteggi come gli aggiornamenti in attesa.

### Status Pill
Stato in parole: pillola piena, icona lucide 12px (tratto 2.5) più parola 12px media, padding 2px 8px 2px 6px. Online: segno di spunta su success-soft; attenzione: cerchio con punto esclamativo su warning-soft; giù: X su destructive-soft; in corso: cerchio di caricamento che ruota solo con motion-safe; spento: pausa; in attesa: cerchio tratteggiato. Sta nella colonna Stato della tabella e accanto al titolo del sito.

### Status Dot
Cerchio pieno di 8px colorato dal livello di salute, con nome accessibile e tooltip. Il blu "in corso" pulsa solo con motion-safe. Compare in barra laterale, palette ⌘K e, a 10px con anello del fondo, sull'angolo dell'avatar nella lista mobile.

### Site Avatar
Tessera di 28px, raggio 6.4px, bordo, fondo crema miele, iniziale del sito 12px semibold. Apre ogni riga di sito.

### Stat Strip
Una riga di quattro celle bordate in un unico contenitore bianco (due colonne su telefono). Sopra: tessera icona di 24px in `bg-honey-soft` con icona 14px ed etichetta 12px muted; sotto: valore 20px semibold tabulare. Il valore si colora solo se c'è un problema (destructive o warning-text).

### Usage Bar
Binario muted alto 6px a pillola, riempimento `bg-honey`, che passa ad arancio attenzione oltre il 75% della soglia d'avviso; a destra la misura in 12px tabulare.

### Row List / Activity List
Lista bordata con divisori, una riga per elemento: pallino, icona o tessera icona (28px, fondo muted) a sinistra, nome in medio e descrizione muted al centro, data 12px a destra. È la forma degli avvisi aperti, dell'attività recente e della lista mobile dei siti.

### Settings Section / Setting Row
Titolo di sezione 14px medio con descrizione 12px muted, poi un riquadro bianco bordato con righe divise da filetti. Ogni riga: etichetta media e spiegazione muted a sinistra, controlli a destra (sotto, su telefono).

### Response Chart
Card con padding 16px: linea di 2px in miele bruciato su un'area che sfuma il miele ambra dal 35% a 0; assi in mono 11px muted. Lo spazio sotto ripete la barra: file in `bg-honey`, database in honey-3.

### Cards / Containers
- **Corner Style:** 8px per i contenitori dell'app, 11.2px per le card della libreria.
- **Background:** card (bianco), sopra la velatura.
- **Shadow Strategy:** nessuna (vedi Elevation & Depth).
- **Border:** 1px border; divisori interni dello stesso colore.
- **Internal Padding:** 12px per 16px per righe e celle, 16px per le card del monitor.

### Inputs / Fields
- **Style:** alti 36px, fondo trasparente, bordo input, raggio 6.4px, ombra minima.
- **Focus:** bordo ambra più anello di 3px in ambra al 50%.
- **Error / Disabled:** bordo e anello destructive con aria-invalid; disabilitato al 50% di opacità.

### Navigation
- **Barra laterale:** voci a 14px con icona lucide da 16px, raggio 6.4px, padding 8px; hover su crema miele, voce attiva su `bg-honey-soft` in peso medio. Le voci dei siti portano il pallino di stato al posto dell'icona. Etichette di gruppo in piccolo e muted.
- **Ricerca ⌘K:** in cima alla barra, un campo finto alto 36px (fondo pagina, bordo, icona di ricerca, "Cerca siti e azioni", tasto ⌘K in mono 11px) che apre la palette dei comandi con siti (pallino e stato) e azioni.
- **Schede del sito:** variante a linea, testo 14px medio al 60%; la scheda attiva va a piena intensità con una linea ambra (primary) di 2px sotto.
- **Telefono:** foglio laterale dal pulsante menu, chiuso a ogni cambio di pagina.

## Do's and Don'ts

### Do:
- **Do** prendere lo stato di un sito da `health()` e mostrarlo con il pallino o con la pillola icona + parola, sempre con la stessa mappa.
- **Do** ordinare i siti con i problemi in testa (giù, attenzione, in corso, in attesa, spento, online).
- **Do** usare `bg-honey` per l'azione principale della vista e `bg-honey-soft` per la selezione, con testo inchiostro pietra.
- **Do** separare i contenitori con fondo card, bordo di 1px e raggio 8px, senza ombra.
- **Do** usare Geist Mono per misure e identificatori e lasciare le cifre tabulari ai dati.
- **Do** dare a ogni tabella una forma a lista per telefono, con le stesse informazioni essenziali.

### Don't:
- **Don't** usare il tema api come illustrazione o decorazione: niente favi, gocce o api disegnate; gli esagoni restano nel marchio.
- **Don't** usare il miele per segnalare un problema, né l'arancio di stato (o le sue varianti -text e -soft) per decorare: miele è azione e marchio, arancio è attenzione.
- **Don't** mettere gradienti fuori dai posti della Miele sulle Azioni Rule (testo, bordi, card intere, fondi di sezione).
- **Don't** usare warning, success o info pieni come colore del testo: usa success-text, warning-text o destructive.
- **Don't** usare metriche finte o banner promozionali da dashboard SaaS generica.
- **Don't** aggiungere ombre a liste, tabelle o strisce dell'app.
- **Don't** aggiungere un clic o uno scroll a un'azione frequente per far posto a un elemento visivo.

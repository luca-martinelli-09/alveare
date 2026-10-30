---
name: Alveare
description: Pannello di hosting self-hosted per siti WordPress e PHP; lo stato prima delle azioni.
colors:
  background: "oklch(1 0 0)"
  foreground: "oklch(0.21 0.006 275)"
  primary: "oklch(0.21 0.006 275)"
  primary-foreground: "oklch(0.985 0 0)"
  secondary: "oklch(0.966 0.002 275)"
  muted: "oklch(0.972 0.002 275)"
  muted-foreground: "oklch(0.5 0.01 275)"
  accent: "oklch(0.958 0.003 275)"
  border: "oklch(0.922 0.004 275)"
  input: "oklch(0.89 0.005 275)"
  ring: "oklch(0.6 0.13 255)"
  sidebar: "oklch(0.981 0.002 275)"
  sidebar-accent: "oklch(0.94 0.004 275)"
  destructive: "oklch(0.57 0.21 25)"
  success: "oklch(0.6 0.15 152)"
  warning: "oklch(0.72 0.16 70)"
  info: "oklch(0.58 0.16 255)"
  brand: "oklch(0.79 0.16 72)"
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
    backgroundColor: "{colors.primary}"
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
  sidebar-item-active:
    backgroundColor: "{colors.sidebar-accent}"
  status-dot:
    rounded: "{rounded.full}"
    size: "8px"
  row-list:
    backgroundColor: "{colors.background}"
    rounded: "{rounded.lg}"
    padding: "12px 16px"
  stat-strip:
    typography: "{typography.title}"
    rounded: "{rounded.lg}"
    padding: "12px 16px"
---

# Design System: Alveare

## Overview

**Creative North Star: "La sala macchine in ordine"**

Alveare è un pannello operativo: chi lo apre vuole sapere in un colpo d'occhio quali siti stanno bene e quali no, poi agire con un clic. Il sistema esegue lo standard della categoria (dashboard di hosting) al livello di Vercel, Railway e della dashboard Cloudflare: neutri freddi quasi monocromi, una barra laterale grigio chiarissimo, filetti sottili al posto delle ombre, Geist Sans per il testo e Geist Mono per misure e identificatori. Il colore è informazione, non decorazione: compare quasi solo come pallino di stato.

La densità è da strumento: corpo a 14px, righe compatte, tabella su desktop e lista a una riga su telefono, con lo stesso ordine (prima i siti con problemi). L'identità sta in due punti soli: il marchio a celle con la cella in cima in ambra, e la disciplina del vocabolario di stato. Tema chiaro e scuro sono pari, scelti dall'utente (Come il sistema, Chiaro, Scuro).

Rifiuti confermati in PRODUCT.md: il tema api o miele come costume, la dashboard SaaS generica (gradienti, metriche finte), i clic o lo scroll in più per le azioni frequenti.

**Key Characteristics:**
- Neutri freddi (tinta 275) quasi acromatici; nessun accento di interfaccia oltre al nero/bianco del primario.
- Ambra di marca confinata nel logo.
- Quattro colori di stato a significato fisso, decisi da una sola regola in codice.
- Profondità con filetti di 1px e lievi scarti tonali, quasi senza ombre.
- Geist Sans e Geist Mono, cifre tabulari nei dati.
- Base shadcn-svelte stile vega, icone lucide.

## Colors

Palette quasi monocroma di grigi freddi, dove gli unici colori saturi portano uno stato o il marchio.

### Primary
- **Inchiostro Grafite** (primary / foreground): testo principale e riempimento del bottone primario ("Nuovo sito", "Crea sito"). In tema scuro si inverte in quasi bianco (oklch(0.96 0.002 275)) con testo grafite.

### Neutral
- **Bianco Pieno** (background): fondo di pagina, card e popover in tema chiaro.
- **Grigio Banco** (sidebar): fondo della barra laterale, un soffio sotto il bianco; in scuro scende sotto il fondo (oklch(0.145 0.005 275)).
- **Grigio Selezione** (sidebar-accent): voce attiva e hover nella barra laterale.
- **Nebbia Fredda** (muted, secondary, accent): hover di righe e bottoni ghost, fondo dei badge secondari, fondo delle iniziali utente, fondo `muted/40` della cornice di accesso.
- **Grigio Didascalia** (muted-foreground): metadati, etichette di colonna, date relative, descrizioni sotto i titoli.
- **Filetto** (border) e **Filetto Campo** (input): bordi di contenitori, divisori di righe, bordi dei campi. In scuro diventano bianco al 9% e 14%.
- **Blu Fuoco** (ring): anello di focus e selezione del testo (info al 22%).

### Stati
- **Verde Online** (success): pallino del sito che risponde.
- **Ambra Attenzione** (warning): pallino "Da guardare" (backup vecchio o fallito, errori 5xx).
- **Rosso Giù** (destructive): pallino del sito che non risponde o in errore; testo di valori falliti; bottoni distruttivi tinti.
- **Blu In Corso** (info): pallino pulsante dei siti in creazione o eliminazione.
- Spento: pallino grigio (muted-foreground al 50%). In attesa del primo controllo: anello vuoto senza riempimento.

### Brand
- **Ambra Cella** (brand): la cella in cima al marchio. Il token esiste in entrambi i temi con lo stesso valore (il logo lo legge con `fill-brand`; la favicon SVG statica ha lo stesso colore in esadecimale); nell'interfaccia non ha altri usi.

### Named Rules
**The Ambra nel Logo Rule.** L'ambra di marca vive solo nel marchio. Mai su bottoni, link, superfici o grafici: lì l'ambra significa "attenzione".

**The Vocabolario di Stato Rule.** Verde online, ambra attenzione, rosso giù o fallito, blu in corso, grigio spento, anello vuoto in attesa. Lo stato si calcola in un solo punto (`ui/src/lib/health.ts`) e ogni superficie (barra laterale, panoramica, sito) lo legge da lì; nessuna pagina ricalcola o ridipinge lo stato a modo suo.

**The Stato Leggibile Rule.** Su fondo chiaro, i colori di stato con contrasto sotto 4.5:1 (warning, success, info) servono per pallini e icone, non per testo. Come testo regge destructive; per l'ambra come testo c'è `warning-text` (oklch(0.52 0.13 60) in chiaro, uguale a warning in scuro), da usare con `text-warning-text`.

## Typography

**Body Font:** Geist Variable (con ui-sans-serif, system-ui)
**Label/Mono Font:** Geist Mono Variable (con ui-monospace)

**Character:** Un sans geometrico e neutro da strumento tecnico, con il suo gemello mono per tutto ciò che si misura o si copia. Nessun font display: la gerarchia la fanno peso e dimensione, non il carattere.

### Hierarchy
- **Headline** (600, 1.5rem/2rem, tracking -0.025em): titolo di pagina nell'intestazione (Panoramica, nome host del sito, Template). Uno per pagina.
- **Title** (600, 1.25rem/1.75rem, cifre tabulari): valori della striscia di stato (Online 4/5, Da guardare 1).
- **Section** (500, 0.875rem/1.25rem): titoli di sezione sopra liste e tabelle ("Avvisi aperti", "Siti"), nomi di sito nelle righe, voce attiva della barra laterale.
- **Body** (400, 0.875rem/1.25rem): testo corrente, celle di tabella, descrizioni. Gli input salgono a 1rem sotto 768px per evitare lo zoom su telefono.
- **Label** (400, 0.75rem/1rem): etichette della striscia di stato, date relative, sottorighe (tipo di sito, "staging"), email nell'account.
- **Mono** (400, 0.75rem/1rem): tempi di risposta ("200 · 84 ms"), identificatori di template, percorsi, campi di identificatore.

### Named Rules
**The Cifre in Colonna Rule.** Celle di tabella, intestazioni, elementi `time` e la classe `.tabular` usano cifre tabulari, impostate globalmente: i numeri si allineano senza lavoro per pagina.

**The Mono per le Misure Rule.** Geist Mono solo per misure, codici e identificatori, mai per titoli o etichette.

## Layout

Barra laterale fissa a sinistra (Panoramica, elenco siti con pallino ordinati per salute, gruppo Amministrazione per gli admin, account in fondo) e colonna di contenuto a destra, centrata con larghezza massima 72rem (max-w-6xl). Margini del contenuto 16px su telefono e 32px da 768px; ritmo verticale tra i blocchi di 24px; dentro una sezione titolo e contenuto distano 8px; righe e celle della striscia hanno 12px verticali e 16px orizzontali.

Sotto 768px la barra laterale diventa un foglio aperto dal pulsante menu in un'intestazione appiccicosa alta 48px (fondo al 90% con sfocatura), e si chiude a ogni navigazione. Sempre sotto 768px la tabella dei siti lascia il posto a una lista a una riga per sito con pallino, nome, primo problema e data del backup; la striscia di stato passa da 4 a 2 colonne sotto 640px. Le schede del sito scorrono in orizzontale senza barra visibile. Il filtro dei siti nella barra laterale compare solo oltre 8 siti.

Le pagine di accesso (login, invito, reset) usano una cornice a parte: fondo muted al 40%, marchio con nome sopra, modulo largo al massimo 24rem al centro.

## Elevation & Depth

Il sistema è piatto: la profondità si legge da filetti di 1px e da scarti tonali minimi (bianco della pagina, grigio banco della barra laterale, nebbia degli hover). I contenitori dell'app (striscia di stato, liste di avvisi e siti, cornice della tabella) hanno bordo e nessuna ombra. Le ombre presenti arrivano dalle primitive vega: un'ombra minima su campi, bottoni outline e card, ombre medie e ampie solo su menu, popover e dialoghi che galleggiano sopra la pagina; card e dialoghi aggiungono un anello foreground al 10%.

### Shadow Vocabulary
- **Appoggio** (`box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05)`, shadow-xs): campi, bottoni outline, card.
- **Sospensione** (shadow-md / shadow-lg di Tailwind): menu a tendina, select, popover.

### Named Rules
**The Filetto Rule.** Un contenitore dell'app si separa con un bordo di 1px nel colore border e i divisori tra righe sono filetti, non ombre. L'ombra resta agli elementi che galleggiano.

## Shapes

Raggio piccolo e coerente, derivato da una sola variabile (0.5rem). I contenitori di pagina (striscia, liste, tabella, schede bordate) usano 8px; bottoni, campi, voci di menu e iniziali utente 6.4px; card e dialoghi 11.2px; i badge sono pillole; il pallino di stato è un cerchio di 8px (10px accanto al titolo del sito). Il marchio è un quadrato arrotondato (raggio 7 su 32) con sei celle esagonali (punta in alto, angoli arrotondati) in impaccamento a nido d'ape, disposte a piramide 1-2-3 come una "A", la cella in cima in ambra.

## Components

### Buttons
Compatti e sobri, altezza 36px, testo 14px medio, con icona lucide a sinistra quando l'azione crea qualcosa.
- **Shape:** angoli lievemente arrotondati (6.4px).
- **Primary:** riempimento grafite e testo bianco (inverso in scuro); hover all'80% di opacità. Per l'azione principale della vista (Nuovo sito, Crea sito).
- **Outline:** fondo pagina, bordo, ombra minima; hover su nebbia.
- **Ghost:** trasparente, hover su nebbia; per azioni di riga e icone.
- **Destructive:** fondo rosso al 10% con testo rosso, non un blocco pieno.
- **Press / Focus:** alla pressione scende di 1px; focus con anello di 3px in ring al 50%.

### Badges
- **Style:** pillola alta 20px, testo 12px medio. Secondary (nebbia) per conteggi come gli aggiornamenti in attesa; destructive tinto per lo stato "Errore"; outline per gli altri stati del ciclo di vita.

### Cards / Containers
- **Corner Style:** 8px per i contenitori dell'app, 11.2px per le card della libreria.
- **Background:** quello della pagina.
- **Shadow Strategy:** nessuna per i contenitori dell'app (vedi Elevation & Depth).
- **Border:** 1px border; divisori interni dello stesso colore.
- **Internal Padding:** 12px per 16px per righe e celle.

### Inputs / Fields
- **Style:** alti 36px, fondo trasparente, bordo input, raggio 6.4px, ombra minima.
- **Focus:** bordo ring più anello di 3px in ring al 50%.
- **Error / Disabled:** bordo e anello destructive con aria-invalid; disabilitato al 50% di opacità.

### Navigation
- **Barra laterale:** voci a 14px con icona lucide da 16px, raggio 6.4px, padding 8px; hover e voce attiva su grigio selezione, attiva anche in peso medio. Le voci dei siti sono compatte e portano il pallino di stato al posto dell'icona. Etichette di gruppo ("Siti", "Amministrazione") in piccolo e muted.
- **Schede del sito:** variante a linea, testo 14px medio al 60%; la scheda attiva va a piena intensità con una linea di 2px sotto.
- **Telefono:** foglio laterale dal pulsante menu, chiuso a ogni cambio di pagina.

### Status Dot
Il componente firma: cerchio pieno di 8px colorato dal livello di salute, con nome accessibile ed etichetta in tooltip. Il blu "in corso" pulsa solo se l'utente non ha chiesto meno movimento. Compare identico in barra laterale, tabella, lista mobile e titolo del sito.

### Stat Strip
Una riga di quattro celle bordate in un unico contenitore (due colonne su telefono): etichetta 12px muted sopra, valore 20px semibold tabulare sotto. Il valore si colora solo se c'è un problema.

### Row List
Lista bordata con divisori, una riga per elemento: pallino o icona a sinistra, nome in medio e sottoriga muted 12px al centro, data o azioni a destra. È la forma mobile della tabella dei siti e la forma degli avvisi aperti.

## Do's and Don'ts

### Do:
- **Do** prendere lo stato di un sito da `health()` e mostrarlo con il pallino, sempre con la stessa mappa di colori.
- **Do** ordinare i siti con i problemi in testa (giù, attenzione, in corso, in attesa, spento, online).
- **Do** separare i contenitori con un bordo di 1px e raggio 8px, senza ombra.
- **Do** usare Geist Mono per misure e identificatori e lasciare le cifre tabulari ai dati.
- **Do** dare a ogni tabella una forma a lista per telefono, con le stesse informazioni essenziali.
- **Do** tenere l'ambra di marca dentro il marchio.

### Don't:
- **Don't** usare il tema api o miele come costume: niente favi, gocce di miele o api decorative nell'interfaccia; il marchio esagonale è l'unico riferimento, e gli esagoni restano nel marchio.
- **Don't** usare gradienti o metriche finte da dashboard SaaS generica.
- **Don't** usare warning, success o info come colore del testo su fondo chiaro: sotto 4.5:1. Per un'avvertenza in testo usa `text-warning-text`.
- **Don't** usare un colore di stato per decorare o per distinguere categorie che non sono stati.
- **Don't** aggiungere ombre a liste, tabelle o strisce dell'app.
- **Don't** aggiungere un clic o uno scroll a un'azione frequente per far posto a un elemento visivo.

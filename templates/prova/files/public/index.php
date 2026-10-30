<?php
// Segnaposto fino al primo deploy: lo sostituisce il public/index.php dell'app.
// Risponde 200: un sito appena creato non deve far scattare l'avviso "non risponde".
echo '<!doctype html><meta charset="utf-8"><title>In arrivo</title><p style="font-family:system-ui;text-align:center;padding:4rem">Applicazione in attesa del primo deploy.</p>';

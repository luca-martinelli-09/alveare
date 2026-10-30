<?php
// Segnaposto fino al primo deploy: lo sostituisce il public/index.php dell'app.
http_response_code(503);
header('Retry-After: 300');
echo '<!doctype html><meta charset="utf-8"><title>In arrivo</title><p style="font-family:system-ui;text-align:center;padding:4rem">Applicazione in attesa del primo deploy.</p>';

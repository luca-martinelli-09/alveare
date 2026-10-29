<?php
/**
 * Identico per tutti i siti di Alveare: nessun segreto qui.
 * Credenziali, chiavi e impostazioni stanno in ../.env, fuori dalla document root.
 */
$alveare_env = [];
foreach (file(dirname(__DIR__) . '/.env', FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES) ?: [] as $line) {
    if (preg_match("/^([A-Z0-9_]+)='(.*)'$/", $line, $m)) {
        $alveare_env[$m[1]] = $m[2];
    }
}
$env = fn (string $key, $default = null) => $alveare_env[$key] ?? $default;

define('DB_NAME', $env('DB_NAME'));
define('DB_USER', $env('DB_USER'));
define('DB_PASSWORD', $env('DB_PASSWORD'));
define('DB_HOST', $env('DB_HOST', 'mariadb'));
define('DB_CHARSET', 'utf8mb4');
define('DB_COLLATE', '');
$table_prefix = $env('TABLE_PREFIX', 'wp_');

foreach (['AUTH_KEY', 'SECURE_AUTH_KEY', 'LOGGED_IN_KEY', 'NONCE_KEY', 'AUTH_SALT', 'SECURE_AUTH_SALT', 'LOGGED_IN_SALT', 'NONCE_SALT'] as $k) {
    define($k, $env($k));
}

define('WP_HOME', $env('WP_HOME'));
define('WP_SITEURL', $env('WP_SITEURL'));
define('WP_ENVIRONMENT_TYPE', $env('WP_ENVIRONMENT_TYPE', 'production'));

// Dietro Cloudflare e Caddy la connessione arriva in HTTP: senza questo, loop di redirect.
if (($_SERVER['HTTP_X_FORWARDED_PROTO'] ?? '') === 'https') {
    $_SERVER['HTTPS'] = 'on';
}

// Il cron lo esegue il pannello; i file appartengono all'utente del sito.
define('DISABLE_WP_CRON', true);
define('FS_METHOD', 'direct');

switch ($env('ALVEARE_UPDATE_RULE', 'security')) {
    case 'auto':
        define('WP_AUTO_UPDATE_CORE', true);
        break;
    case 'manual':
        define('AUTOMATIC_UPDATER_DISABLED', true);
        break;
    default:
        define('WP_AUTO_UPDATE_CORE', 'minor');
}

define('WP_DEBUG', $env('WP_DEBUG') === 'true');
define('WP_DEBUG_LOG', $env('WP_DEBUG') === 'true');
define('WP_DEBUG_DISPLAY', false);

foreach (['SMTP_HOST', 'SMTP_PORT', 'SMTP_USER', 'SMTP_PASSWORD', 'SMTP_FROM', 'SMTP_FROM_NAME', 'ALVEARE_UPDATE_RULE'] as $k) {
    if ($env($k) !== null) {
        define($k, $env($k));
    }
}

if (!defined('ABSPATH')) {
    define('ABSPATH', __DIR__ . '/');
}
require_once ABSPATH . 'wp-settings.php';

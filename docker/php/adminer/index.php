<?php
/**
 * Adminer aperto dal pannello: un token monouso (scritto dal pannello, valido 5 minuti)
 * apre la sessione sul solo database del sito. Senza token non si entra, nemmeno a mano:
 * niente login con altre credenziali né connessioni verso altri server.
 */
session_name('alveare_adminer');
$https = ($_SERVER['HTTP_X_FORWARDED_PROTO'] ?? '') === 'https';
session_set_cookie_params(['httponly' => true, 'samesite' => 'Lax', 'secure' => $https, 'path' => '/']);
session_start();

$token = $_GET['alveare'] ?? '';
if ($token !== '') {
    if (!preg_match('/^[A-Za-z0-9]{40}$/', $token)) {
        http_response_code(400);
        exit('Link non valido.');
    }
    $file = "/srv/adminer/tokens/$token.json";
    $grant = is_file($file) ? json_decode((string) file_get_contents($file), true) : null;
    @unlink($file);
    if (!is_array($grant) || ($grant['exp'] ?? 0) < time()) {
        http_response_code(403);
        exit('Link scaduto: riapri Adminer dal pannello.');
    }
    session_regenerate_id(true);
    $_SESSION['alveare'] = $grant;
    $_POST = ['auth' => [
        'driver' => 'server',
        'server' => $grant['host'],
        'username' => $grant['user'],
        'password' => $grant['password'],
        'db' => $grant['db'],
        'permanent' => 0,
    ]];
    $_SERVER['REQUEST_METHOD'] = 'POST';
} elseif (empty($_SESSION['alveare'])) {
    http_response_code(403);
    exit('Apri Adminer dal pannello di Alveare.');
}

function adminer_object()
{
    class AlveareAdminer extends \Adminer\Adminer
    {
        public function name()
        {
            return 'Alveare · ' . htmlspecialchars($_SESSION['alveare']['db']);
        }

        public function credentials()
        {
            $g = $_SESSION['alveare'];
            return [$g['host'], $g['user'], $g['password']];
        }

        public function login($login, $password)
        {
            return $login === ($_SESSION['alveare']['user'] ?? null) ?: 'Apri Adminer dal pannello.';
        }

        // Il login arriva dal token, non da un form: niente token CSRF da verificare.
        public function verifyLoginToken()
        {
            return false;
        }

        public function databases($flush = true)
        {
            return [$_SESSION['alveare']['db']];
        }
    }

    return new AlveareAdminer();
}

include __DIR__ . '/adminer.php';

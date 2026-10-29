<?php
/**
 * Adminer aperto dal pannello: un token monouso (scritto dal pannello, valido 5 minuti)
 * apre la sessione sul solo database del sito. Senza token non si entra, nemmeno a mano:
 * niente login con altre credenziali né connessioni verso altri server.
 *
 * Il token si gestisce in adminer_object(): Adminer la chiama dopo aver aperto la sua
 * sessione e prima di elaborare il login.
 */
if (($_SERVER['HTTP_X_FORWARDED_PROTO'] ?? '') === 'https') {
    $_SERVER['HTTPS'] = 'on';
}

function alveare_deny(int $status, string $message): void
{
    http_response_code($status);
    header('Content-Type: text/plain; charset=utf-8');
    exit($message);
}

function adminer_object()
{
    $token = $_GET['alveare'] ?? '';
    if ($token !== '') {
        if (!preg_match('/^[A-Za-z0-9]{40}$/', $token)) {
            alveare_deny(400, 'Link non valido.');
        }
        $file = "/srv/adminer/tokens/$token.json";
        $grant = is_file($file) ? json_decode((string) file_get_contents($file), true) : null;
        @unlink($file);
        if (!is_array($grant) || ($grant['exp'] ?? 0) < time()) {
            alveare_deny(403, 'Link scaduto: riapri Adminer dal pannello.');
        }
        $_SESSION['alveare'] = $grant;
        // Adminer costruisce il redirect dall'URL corrente: il token consumato non deve restarci.
        unset($_GET['alveare']);
        $_SERVER['REQUEST_URI'] = preg_replace('/([?&])alveare=[^&]*&?/', '$1', $_SERVER['REQUEST_URI']);
        $_POST = ['auth' => [
            'driver' => 'server',
            'server' => $grant['host'],
            'username' => $grant['user'],
            'password' => $grant['password'],
            'db' => $grant['db'],
            'permanent' => 0,
        ]];
    } elseif (empty($_SESSION['alveare'])) {
        alveare_deny(403, 'Apri Adminer dal pannello di Alveare.');
    }

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

<?php
/**
 * Plugin Name: Alveare
 * Description: Email via SMTP (AWS SES) e regola di aggiornamento decise dal pannello.
 */

add_action('phpmailer_init', function ($mail) {
    if (!defined('SMTP_HOST')) {
        return;
    }
    $mail->isSMTP();
    $mail->Host = SMTP_HOST;
    $mail->Port = defined('SMTP_PORT') ? (int) SMTP_PORT : 587;
    $mail->SMTPAuth = defined('SMTP_USER');
    $mail->Username = defined('SMTP_USER') ? SMTP_USER : '';
    $mail->Password = defined('SMTP_PASSWORD') ? SMTP_PASSWORD : '';
    $mail->SMTPSecure = 'tls';
    if (defined('SMTP_FROM')) {
        $mail->setFrom(SMTP_FROM, defined('SMTP_FROM_NAME') ? SMTP_FROM_NAME : '', false);
    }
});

// Plugin e temi si aggiornano da soli solo con la regola "automatico".
$alveare_auto = defined('ALVEARE_UPDATE_RULE') && ALVEARE_UPDATE_RULE === 'auto';
add_filter('auto_update_plugin', fn () => $alveare_auto);
add_filter('auto_update_theme', fn () => $alveare_auto);

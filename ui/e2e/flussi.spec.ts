// Flussi principali dal browser: accesso con TOTP, crea sito, carica un file, legge i log.
import { test, expect, type Page } from '@playwright/test';
import { createHmac } from 'node:crypto';
import { readFileSync } from 'node:fs';

type Config = { url: string; email: string; password: string; totp_secret: string };
const cfg: Config = JSON.parse(readFileSync(process.env.E2E_CONFIG ?? 'e2e.json', 'utf8'));

function totp(secret: string, at = Date.now()): string {
	const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
	let bits = '';
	for (const c of secret.replace(/=+$/, '')) bits += alphabet.indexOf(c).toString(2).padStart(5, '0');
	const key = Buffer.from(bits.match(/.{8}/g)!.map((b) => parseInt(b, 2)));
	const counter = Buffer.alloc(8);
	counter.writeBigUInt64BE(BigInt(Math.floor(at / 30000)));
	const h = createHmac('sha1', key).update(counter).digest();
	const off = h[h.length - 1] & 0xf;
	return String((h.readUInt32BE(off) & 0x7fffffff) % 1_000_000).padStart(6, '0');
}

async function login(page: Page) {
	await page.goto(`${cfg.url}/login`);
	await page.getByLabel('Email').fill(cfg.email);
	await page.getByLabel('Password').fill(cfg.password);
	await page.getByRole('button', { name: 'Accedi', exact: true }).click();
	// Un codice vale una volta: si aspetta il passo successivo per non riusare quello del test precedente.
	await page.waitForTimeout(30_000 - (Date.now() % 30_000) + 500);
	await page.getByRole('textbox').fill(totp(cfg.totp_secret));
	await page.getByRole('button', { name: /Conferma|Verifica|Accedi/ }).click();
	await expect(page.getByRole('heading', { name: 'Siti' })).toBeVisible();
}

test('accesso, nuovo sito, file manager e log', async ({ page }) => {
	await login(page);

	await page.getByRole('button', { name: 'Nuovo sito' }).click();
	await page.getByLabel('Dominio').fill('browser.test');
	await page.getByLabel('Titolo').fill('Dal browser');
	await page.getByRole('button', { name: 'Crea sito' }).click();
	await expect(page.getByText('Completato')).toBeVisible({ timeout: 150_000 });
	await page.keyboard.press('Escape');
	await page.getByRole('cell', { name: 'browser.test' }).click();
	await expect(page.getByRole('heading', { name: 'browser.test' })).toBeVisible();

	await page.getByRole('tab', { name: 'File' }).click();
	await page.locator('input[type=file]').first().setInputFiles({ name: 'ciao.txt', mimeType: 'text/plain', buffer: Buffer.from('ciao dal browser') });
	await expect(page.getByRole('button', { name: 'ciao.txt' })).toBeVisible();

	await page.getByRole('tab', { name: 'Log' }).click();
	await expect(page.getByText('Accessi', { exact: true })).toBeVisible();
	await page.getByText('Errori PHP', { exact: true }).click();
	await expect(page.getByRole('button', { name: 'Raggruppa' })).toBeVisible();
});

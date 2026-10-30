// Stato di un sito in una parola, per la barra laterale e la panoramica: una regola sola.
import type { SiteRow } from './api';

export type Level = 'ok' | 'warn' | 'down' | 'busy' | 'off' | 'unknown';
export type Health = { level: Level; label: string; issues: string[] };

export const BACKUP_STALE_H = 36;

export function health(site: SiteRow, now = Date.now()): Health {
	if (site.status === 'creating') return { level: 'busy', label: 'In creazione', issues: [] };
	if (site.status === 'deleting') return { level: 'busy', label: 'In eliminazione', issues: [] };
	if (site.status === 'suspended') return { level: 'off', label: 'Sospeso', issues: [] };
	if (site.status === 'error')
		return {
			level: 'down',
			label: 'Errore',
			issues: ['creazione o operazione fallita']
		};
	const issues: string[] = [];
	const b = site.last_backup;
	if (site.backups && !b) issues.push('nessun backup ancora');
	else if (b?.status === 'failed') issues.push('ultimo backup fallito');
	else if (b && now - Date.parse(b.started_at) > BACKUP_STALE_H * 3600_000) issues.push('backup più vecchio di 36 ore');
	const errors = site.monitor?.traffic?.errors_5xx ?? 0;
	if (errors > 0) issues.push(`${errors} errori 5xx nell'ultima ora`);
	if (site.online === false) {
		const u = site.monitor?.uptime;
		return {
			level: 'down',
			label: 'Non risponde',
			issues: [u?.error ?? `risposta ${u?.status ?? '?'}`, ...issues]
		};
	}
	if (issues.length) return { level: 'warn', label: 'Da guardare', issues };
	if (site.online === null) return { level: 'unknown', label: 'In attesa del primo controllo', issues };
	return { level: 'ok', label: 'Online', issues };
}

/** Prima i siti con problemi, poi per dominio. */
const rank: Record<Level, number> = {
	down: 0,
	warn: 1,
	busy: 2,
	unknown: 3,
	off: 4,
	ok: 5
};
export function byHealth(a: SiteRow, b: SiteRow) {
	return rank[health(a).level] - rank[health(b).level] || (a.domain ?? a.slug).localeCompare(b.domain ?? b.slug);
}

export function ago(iso: string | number | null | undefined, now = Date.now()): string {
	if (iso === null || iso === undefined) return '–';
	const t = typeof iso === 'number' ? iso * 1000 : Date.parse(iso);
	const m = Math.round((now - t) / 60_000);
	if (m < 1) return 'adesso';
	if (m < 60) return `${m} min fa`;
	const h = Math.round(m / 60);
	if (h < 48) return `${h} h fa`;
	return `${Math.round(h / 24)} giorni fa`;
}

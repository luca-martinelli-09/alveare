// Tabella dei siti nella panoramica: filtri, ordinamento per colonna e pagine, tutto nel browser
// (i siti di un server sono decine, non migliaia).
import type { SiteRow } from './api';
import { health, rank } from './health.ts';

export type SortKey = 'site' | 'status' | 'response' | 'backup' | 'space' | 'requests';
export type SortDir = 'asc' | 'desc';
export type StatusFilter = 'all' | 'problems' | 'ok' | 'off';
export type Filters = { q: string; status: StatusFilter; kind: string };

const name = (s: SiteRow) => s.domain ?? s.slug;

export function filterSites(list: SiteRow[], f: Filters, now = Date.now()): SiteRow[] {
	const q = f.q.trim().toLowerCase();
	return list.filter((s) => {
		if (q && !name(s).toLowerCase().includes(q) && !s.slug.includes(q)) return false;
		if (f.kind !== 'all' && s.kind !== f.kind) return false;
		const level = health(s, now).level;
		if (f.status === 'problems') return level === 'down' || level === 'warn';
		if (f.status === 'ok') return level === 'ok';
		if (f.status === 'off') return level === 'off';
		return true;
	});
}

/** Valore da ordinare; `null` = manca, e va sempre in fondo. */
function value(s: SiteRow, key: SortKey, now: number): number | string | null {
	const m = s.monitor;
	switch (key) {
		case 'site':
			return name(s);
		case 'status':
			return rank[health(s, now).level];
		case 'response':
			return m?.uptime?.ms ?? null;
		case 'backup':
			return s.last_backup ? Date.parse(s.last_backup.started_at) : null;
		case 'space':
			return m?.usage ? (m.usage.files_bytes ?? 0) + (m.usage.db_bytes ?? 0) : null;
		case 'requests':
			return m?.traffic?.requests ?? null;
	}
}

export function sortSites(list: SiteRow[], key: SortKey, dir: SortDir, now = Date.now()): SiteRow[] {
	const sign = dir === 'asc' ? 1 : -1;
	return list.toSorted((a, b) => {
		const [va, vb] = [value(a, key, now), value(b, key, now)];
		if (va === null || vb === null) return va === vb ? name(a).localeCompare(name(b)) : va === null ? 1 : -1;
		const c = typeof va === 'string' ? va.localeCompare(vb as string) : va - (vb as number);
		return c * sign || name(a).localeCompare(name(b));
	});
}

export function paginate<T>(list: T[], page: number, size: number) {
	const pages = Math.max(1, Math.ceil(list.length / size));
	const p = Math.min(Math.max(1, page), pages);
	const start = (p - 1) * size;
	const rows = list.slice(start, start + size);
	return { rows, page: p, pages, total: list.length, from: rows.length ? start + 1 : 0, to: start + rows.length };
}

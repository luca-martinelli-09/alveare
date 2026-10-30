// node --test src/lib/site-table.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { filterSites, sortSites, paginate } from './site-table.ts';
import type { SiteRow } from './api';

const NOW = Date.parse('2026-10-30T12:00:00Z');
let next = 1;
const site = (domain: string, over: Partial<SiteRow> = {}): SiteRow => ({
	id: next++,
	slug: domain.replace(/\./g, '_'),
	domain,
	kind: 'wordpress',
	status: 'active',
	online: true,
	staging: false,
	monitor: { uptime: { ok: true, status: 200, ms: 100 }, usage: null, traffic: null },
	last_backup: { started_at: '2026-10-30T02:00:00Z', status: 'done' },
	backups: true,
	url: `https://${domain}`,
	disk_alert_mb: 10240,
	...over
});

const sona = site('sona.it', { monitor: { uptime: { ok: true, status: 200, ms: 300 }, usage: { files_bytes: 5, db_bytes: 5 }, traffic: { requests: 40 } } });
const upupa = site('upupa.it', { online: false, monitor: { uptime: { ok: false, status: 502 }, usage: null, traffic: null } });
const app = site('app.epops.it', { kind: 'laravel', monitor: { uptime: { ok: true, status: 200, ms: 50 }, usage: { files_bytes: 100, db_bytes: 0 }, traffic: { requests: 2 } }, last_backup: null });
const pausa = site('pausa.it', { status: 'suspended' });
const all = [sona, upupa, app, pausa];
const names = (l: SiteRow[]) => l.map((s) => s.domain);

test('filtri: testo sul dominio, stato e tipo', () => {
	assert.deepEqual(names(filterSites(all, { q: 'EPOPS', status: 'all', kind: 'all' }, NOW)), ['app.epops.it']);
	assert.deepEqual(names(filterSites(all, { q: '', status: 'problems', kind: 'all' }, NOW)), ['upupa.it', 'app.epops.it'], 'non risponde e nessun backup');
	assert.deepEqual(names(filterSites(all, { q: '', status: 'off', kind: 'all' }, NOW)), ['pausa.it']);
	assert.deepEqual(names(filterSites(all, { q: '', status: 'all', kind: 'laravel' }, NOW)), ['app.epops.it']);
});

test('ordinamento per colonna, i valori mancanti sempre in fondo', () => {
	assert.deepEqual(names(sortSites(all, 'status', 'asc', NOW)), ['upupa.it', 'app.epops.it', 'pausa.it', 'sona.it'], 'prima i problemi');
	assert.deepEqual(names(sortSites(all, 'site', 'desc', NOW)), ['upupa.it', 'sona.it', 'pausa.it', 'app.epops.it']);
	assert.deepEqual(names(sortSites(all, 'response', 'asc', NOW)).slice(0, 3), ['app.epops.it', 'pausa.it', 'sona.it']);
	assert.equal(names(sortSites(all, 'response', 'desc', NOW)).at(-1), 'upupa.it', 'senza tempo di risposta: in fondo anche al contrario');
	assert.deepEqual(names(sortSites(all, 'requests', 'desc', NOW)).slice(0, 2), ['sona.it', 'app.epops.it']);
	assert.deepEqual(names(sortSites(all, 'space', 'desc', NOW)).slice(0, 2), ['app.epops.it', 'sona.it']);
	assert.equal(names(sortSites(all, 'backup', 'desc', NOW)).at(-1), 'app.epops.it', 'mai fatto: in fondo');
});

test('pagine: righe, totale e pagina oltre la fine riportata all\'ultima', () => {
	const many = Array.from({ length: 23 }, (_, i) => site(`s${i}.it`));
	const p = paginate(many, 1, 10);
	assert.deepEqual([p.rows.length, p.page, p.pages, p.from, p.to, p.total], [10, 1, 3, 1, 10, 23]);
	const last = paginate(many, 9, 10);
	assert.deepEqual([last.rows.length, last.page, last.from, last.to], [3, 3, 21, 23]);
	assert.deepEqual([paginate([], 1, 10).pages, paginate([], 1, 10).from], [1, 0]);
});

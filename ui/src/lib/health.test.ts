// node --test src/lib/health.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { health, byHealth } from './health.ts';
import type { SiteRow } from './api';

const NOW = Date.parse('2026-10-30T12:00:00Z');
const site = (over: Partial<SiteRow>): SiteRow => ({
	id: 1,
	slug: 'a',
	domain: 'a.it',
	kind: 'wordpress',
	status: 'active',
	online: true,
	staging: false,
	monitor: { uptime: { ok: true, status: 200 }, usage: null, traffic: null },
	last_backup: { started_at: '2026-10-30T02:00:00Z', status: 'done' },
	backups: true,
	url: 'https://a.it',
	...over
});

test('un sito sano è online', () => {
	assert.equal(health(site({}), NOW).level, 'ok');
});

test('non risponde vince sugli altri problemi e ne dice il motivo', () => {
	const h = health(site({ online: false, monitor: { uptime: { ok: false, status: 502 }, usage: null, traffic: { errors_5xx: 3 } } }), NOW);
	assert.equal(h.level, 'down');
	assert.deepEqual(h.issues, ['risposta 502', "3 errori 5xx nell'ultima ora"]);
});

test('backup fallito o vecchio: da guardare', () => {
	assert.equal(health(site({ last_backup: { started_at: '2026-10-30T02:00:00Z', status: 'failed' } }), NOW).level, 'warn');
	assert.equal(health(site({ last_backup: { started_at: '2026-10-28T02:00:00Z', status: 'done' } }), NOW).level, 'warn');
});

test('nessun backup: da guardare, ma solo se i backup sono configurati', () => {
	assert.deepEqual(health(site({ last_backup: null }), NOW).issues, ['nessun backup ancora']);
	assert.equal(health(site({ last_backup: null, backups: false }), NOW).level, 'ok');
});

test('sospeso e in creazione non sono guasti', () => {
	assert.equal(health(site({ status: 'suspended', online: false }), NOW).level, 'off');
	assert.equal(health(site({ status: 'creating', online: null }), NOW).level, 'busy');
});

test('ordine: prima i problemi, poi per dominio', () => {
	const list = [site({ id: 1, domain: 'b.it' }), site({ id: 2, domain: 'z.it', online: false }), site({ id: 3, domain: 'a.it' })];
	assert.deepEqual(list.toSorted(byHealth).map((s) => s.domain), ['z.it', 'a.it', 'b.it']);
});

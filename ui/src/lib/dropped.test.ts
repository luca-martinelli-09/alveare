// node --test src/lib/dropped.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { filesFromEntries, type Entry } from './dropped.ts';

const file = (name: string): Entry => ({ isFile: true, isDirectory: false, name, file: (ok) => ok({ name } as File) });
// Come i browser: readEntries restituisce a blocchi e poi un blocco vuoto.
const dir = (name: string, children: Entry[], batch = 2): Entry => ({
	isFile: false,
	isDirectory: true,
	name,
	createReader: () => {
		let i = 0;
		return { readEntries: (ok) => ok(children.slice(i, (i += batch))) };
	}
});

test('cartelle trascinate: tutti i file con il percorso relativo, anche oltre il primo blocco', async () => {
	const tema = dir('tema', [file('style.css'), file('functions.php'), dir('css', [file('a.css')]), file('screenshot.png')]);
	const out = await filesFromEntries([tema, file('readme.txt')]);
	assert.deepEqual(out.map((f) => f.path).sort(), ['readme.txt', 'tema/css/a.css', 'tema/functions.php', 'tema/screenshot.png', 'tema/style.css']);
});

test('una cartella vuota non produce file', async () => {
	assert.deepEqual(await filesFromEntries([dir('vuota', [])]), []);
});

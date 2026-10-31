// node --test src/lib/upload-queue.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { summary, type UploadItem } from './upload-queue.ts';

const item = (name: string, size: number, status: UploadItem['status'], progress = 0): UploadItem => ({ id: name, name, size, status, progress, error: null });

test('conta fatti, errori, in attesa e il file in corso; percentuale pesata sui byte', () => {
	const s = summary([item('a.jpg', 100, 'done', 1), item('b.mp4', 900, 'uploading', 0.5), item('c.txt', 0, 'waiting'), item('d.pdf', 0, 'error')]);
	assert.deepEqual(
		{ total: s.total, done: s.done, failed: s.failed, waiting: s.waiting, current: s.current?.name, percent: s.percent, finished: s.finished },
		{ total: 4, done: 1, failed: 1, waiting: 1, current: 'b.mp4', percent: 55, finished: false }
	);
});

test('i saltati contano a parte e non pesano sulla percentuale', () => {
	const s = summary([item('a', 100, 'done', 1), item('b', 100, 'skipped'), item('c', 100, 'uploading', 0.5)]);
	assert.deepEqual({ done: s.done, skipped: s.skipped, percent: s.percent, finished: s.finished }, { done: 1, skipped: 1, percent: 75, finished: false });
	assert.equal(summary([item('b', 100, 'skipped')]).finished, true);
});

test('finita quando nessuno è in attesa o in corso; coda vuota a 0%', () => {
	assert.equal(summary([item('a', 10, 'done', 1), item('b', 10, 'error')]).finished, true);
	assert.deepEqual({ ...summary([]) }, { total: 0, done: 0, failed: 0, skipped: 0, waiting: 0, current: null, percent: 0, finished: true });
	assert.equal(summary([item('vuoto', 0, 'done', 1)]).percent, 100, 'file da 0 byte: conta come fatto');
});

test('un file in attesa di risposta tiene aperta la coda e conta fra quelli in attesa', () => {
	const s = summary([item('a', 10, 'done', 1), item('b', 10, 'conflict')]);
	assert.deepEqual({ waiting: s.waiting, finished: s.finished }, { waiting: 1, finished: false });
});

// Upload del file manager in coda, uno alla volta, con lo stato di ogni file per il pannello in basso
// a destra. Sta fuori dalla pagina: se cambi scheda o sito il caricamento continua e resta visibile.
import { uploadTo } from './upload';
import { errorMessage } from './api';
import type { UploadItem } from './upload-queue';
import type { Dropped } from './dropped';

export const uploads = $state<{ items: UploadItem[]; minimized: boolean }>({ items: [], minimized: false });

// I File restano fuori dallo stato reattivo: servono solo a chi carica.
const tasks = new Map<string, { url: string; path: string; file: File; onDone?: () => void }>();
let running = false;

/** Mette in coda i file (`path` relativo a `dir`); `onDone` quando la coda si svuota, per ricaricare l'elenco. */
export function enqueue(url: string, files: Dropped[], dir: string, onDone?: () => void) {
	for (const { file, path } of files) {
		const id = crypto.randomUUID();
		tasks.set(id, { url, path: dir ? `${dir}/${path}` : path, file, onDone });
		uploads.items.push({ id, name: path, size: file.size, status: 'waiting', progress: 0, error: null });
	}
	uploads.minimized = false;
	if (!running) void work();
}

async function work() {
	running = true;
	const done = new Set<() => void>();
	for (let item = next(); item; item = next()) {
		const task = tasks.get(item.id)!;
		item.status = 'uploading';
		try {
			await uploadTo(task.url, task.path, task.file, (f) => (item.progress = f));
			[item.status, item.progress] = ['done', 1];
		} catch (e) {
			[item.status, item.error] = ['error', errorMessage(e)];
		}
		tasks.delete(item.id);
		if (task.onDone) done.add(task.onDone);
	}
	running = false;
	done.forEach((f) => f());
}

const next = () => uploads.items.find((i) => i.status === 'waiting');

/** Chiude il pannello a coda finita. */
export function clearUploads() {
	if (!running) uploads.items = [];
}

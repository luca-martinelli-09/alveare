// Upload del file manager in coda, uno alla volta, con lo stato di ogni file per il pannello in basso
// a destra. Sta fuori dalla pagina: se cambi scheda o sito il caricamento continua e resta visibile.
import { uploadTo } from './upload';
import { ApiError, errorMessage } from './api';
import type { UploadItem } from './upload-queue';
import type { Dropped } from './dropped';

/** Domanda aperta su un file che c'è già: la risposta arriva a `resolve`. */
export type Conflict = { name: string; others: number; resolve: (r: { replace: boolean; all: boolean }) => void };

export const uploads = $state<{ items: UploadItem[]; minimized: boolean; conflict: Conflict | null }>({ items: [], minimized: false, conflict: null });

// Un gruppo per ogni caricamento (un trascinamento, un "Carica"): lì vale "Applica a tutti".
type Batch = { replace: boolean | null };
// I File restano fuori dallo stato reattivo: servono solo a chi carica.
const tasks = new Map<string, { url: string; path: string; file: File; batch: Batch; onDone?: () => void }>();
let running = false;

/** Mette in coda i file (`path` relativo a `dir`); `onDone` quando la coda si svuota, per ricaricare l'elenco. */
export function enqueue(url: string, files: Dropped[], dir: string, onDone?: () => void) {
	const batch: Batch = { replace: null };
	for (const { file, path } of files) {
		const id = crypto.randomUUID();
		tasks.set(id, { url, path: dir ? `${dir}/${path}` : path, file, batch, onDone });
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
		const progress = (f: number) => (item.progress = f);
		try {
			try {
				await uploadTo(task.url, task.path, task.file, progress, false);
			} catch (e) {
				if (!(e instanceof ApiError && e.status === 409)) throw e;
				// C'è già: si chiede (o vale la risposta data con "Applica a tutti").
				if (!(task.batch.replace ?? (await ask(item.name, task.batch)))) {
					item.status = 'skipped';
					continue;
				}
				await uploadTo(task.url, task.path, task.file, progress, true);
			}
			[item.status, item.progress] = ['done', 1];
		} catch (e) {
			[item.status, item.error] = ['error', errorMessage(e)];
		} finally {
			tasks.delete(item.id);
			if (task.onDone) done.add(task.onDone);
		}
	}
	running = false;
	done.forEach((f) => f());
}

/** Sostituire `name`? Con "Applica a tutti" la scelta resta per il resto del gruppo. */
async function ask(name: string, batch: Batch): Promise<boolean> {
	const others = [...tasks.values()].filter((t) => t.batch === batch).length - 1;
	const r = await new Promise<{ replace: boolean; all: boolean }>((resolve) => (uploads.conflict = { name, others, resolve }));
	uploads.conflict = null;
	if (r.all) batch.replace = r.replace;
	return r.replace;
}

const next = () => uploads.items.find((i) => i.status === 'waiting');

/** Chiude il pannello a coda finita. */
export function clearUploads() {
	if (!running) uploads.items = [];
}

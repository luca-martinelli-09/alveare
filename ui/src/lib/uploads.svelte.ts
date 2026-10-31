// Upload del file manager in coda, qualcuno alla volta, con lo stato di ogni file per il pannello in basso
// a destra. Sta fuori dalla pagina: se cambi scheda o sito il caricamento continua e resta visibile.
import { uploadTo } from './upload';
import { ApiError, errorMessage } from './api';
import type { UploadItem } from './upload-queue';
import type { Dropped } from './dropped';

/** Domanda aperta su un file che c'è già: la risposta arriva a `resolve`. */
export type Conflict = { name: string; others: number; resolve: (r: { replace: boolean; all: boolean }) => void };

export const uploads = $state<{ items: UploadItem[]; minimized: boolean; conflict: Conflict | null }>({ items: [], minimized: false, conflict: null });

/** File caricati insieme: tanti file piccoli sono lenti per i giri di rete, non per i byte. */
const PARALLEL = 4;

// Un gruppo per ogni caricamento (un trascinamento, un "Carica"): lì vale "Applica a tutti".
type Batch = { replace: boolean | null };
type Task = { url: string; path: string; file: File; batch: Batch; overwrite: boolean; onDone?: () => void };
// I File restano fuori dallo stato reattivo: servono solo a chi carica.
const tasks = new Map<string, Task>();
let active = 0;
// Le domande "c'è già" arrivano una alla volta, mentre gli altri file continuano a caricarsi.
let questions: Promise<void> = Promise.resolve();
const done = new Set<() => void>();

/** Mette in coda i file (`path` relativo a `dir`); `onDone` quando la coda si ferma, per ricaricare l'elenco. */
export function enqueue(url: string, files: Dropped[], dir: string, onDone?: () => void) {
	const batch: Batch = { replace: null };
	for (const { file, path } of files) {
		const id = crypto.randomUUID();
		tasks.set(id, { url, path: dir ? `${dir}/${path}` : path, file, batch, overwrite: false, onDone });
		uploads.items.push({ id, name: path, size: file.size, status: 'waiting', progress: 0, error: null });
	}
	uploads.minimized = false;
	kick();
}

function kick() {
	for (let item = next(); item && active < PARALLEL; item = next()) void upload(item);
	if (active === 0 && !next()) {
		done.forEach((f) => f());
		done.clear();
	}
}

async function upload(item: UploadItem) {
	const task = tasks.get(item.id)!;
	active++;
	item.status = 'uploading';
	try {
		await uploadTo(task.url, task.path, task.file, (f) => (item.progress = f), task.overwrite);
		[item.status, item.progress] = ['done', 1];
	} catch (e) {
		if (e instanceof ApiError && e.status === 409 && !task.overwrite) {
			item.status = 'conflict';
			decide(item, task);
		} else [item.status, item.error] = ['error', errorMessage(e)];
	} finally {
		if (item.status !== 'conflict') finish(item, task);
		active--;
		kick();
	}
}

/** C'è già: vale la risposta data con "Applica a tutti", altrimenti si chiede (dopo le domande prima). */
function decide(item: UploadItem, task: Task) {
	const apply = (replace: boolean) => {
		if (replace) [task.overwrite, item.status, item.progress] = [true, 'waiting', 0];
		else [item.status, item.progress] = ['skipped', 0];
		if (!replace) finish(item, task);
		kick();
	};
	if (task.batch.replace !== null) return apply(task.batch.replace);
	questions = questions.then(async () => apply(task.batch.replace ?? (await ask(item.name, task.batch))));
}

function finish(item: UploadItem, task: Task) {
	tasks.delete(item.id);
	if (task.onDone) done.add(task.onDone);
}

/** Sostituire `name`? Con "Applica a tutti" la scelta resta per il resto del gruppo. */
async function ask(name: string, batch: Batch): Promise<boolean> {
	const others = [...tasks.values()].filter((t) => t.batch === batch).length - 1;
	const r = await new Promise<{ replace: boolean; all: boolean }>((resolve) => {
		uploads.conflict = { name, others, resolve };
	});
	uploads.conflict = null;
	if (r.all) batch.replace = r.replace;
	return r.replace;
}

const next = () => uploads.items.find((i) => i.status === 'waiting');

/** Chiude il pannello a coda finita. */
export function clearUploads() {
	if (tasks.size === 0) uploads.items = [];
}

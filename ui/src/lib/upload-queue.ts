// Coda degli upload del file manager: lo stato di ogni file e il riepilogo per il pannello in basso a destra.
export type UploadItem = {
	id: string;
	name: string;
	size: number;
	status: 'waiting' | 'uploading' | 'done' | 'error';
	/** Da 0 a 1. */
	progress: number;
	error: string | null;
};

export function summary(items: UploadItem[]) {
	const count = (s: UploadItem['status']) => items.filter((i) => i.status === s).length;
	// I falliti non contano nella percentuale: non arriveranno mai al 100%.
	const live = items.filter((i) => i.status !== 'error');
	const bytes = live.reduce((a, i) => a + i.size, 0);
	const sent = live.reduce((a, i) => a + i.size * i.progress, 0);
	const percent = bytes > 0 ? Math.round((sent / bytes) * 100) : live.length ? Math.round((count('done') / live.length) * 100) : 0;
	return {
		total: items.length,
		done: count('done'),
		failed: count('error'),
		waiting: count('waiting'),
		current: items.find((i) => i.status === 'uploading') ?? null,
		percent,
		finished: count('waiting') === 0 && count('uploading') === 0
	};
}

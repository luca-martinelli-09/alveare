// Upload a blocchi: Cloudflare limita una richiesta a 100 MB, il pannello ricompone i pezzi.
import { api } from './api';

type UploadInfo = { id: string; chunk_size: number; chunks: number };
type Status = { received: number[]; complete: boolean };

/** Upload su un'area: `/api/sites/<id>/files/uploads`, `/api/templates/<nome>/files/uploads`, `/api/imports/uploads`. */
/** `overwrite: false`: se il file c'è già il server risponde 409 senza toccarlo (il file manager chiede). */
export async function uploadTo(base: string, path: string, file: File, onProgress: (fraction: number) => void, overwrite = true) {
	const info = await api.post<UploadInfo>(base, { path, size: file.size, overwrite });
	for (let n = 0; n < info.chunks; n++) {
		const blob = file.slice(n * info.chunk_size, (n + 1) * info.chunk_size);
		// Un blocco fallito si ritenta: il server accetta lo stesso blocco più volte.
		for (let attempt = 1; ; attempt++) {
			try {
				await api.put<Status>(`${base}/${info.id}/${n}`, blob);
				break;
			} catch (e) {
				if (attempt >= 3) throw e;
				await new Promise((r) => setTimeout(r, 1000 * attempt));
			}
		}
		onProgress((n + 1) / info.chunks);
	}
}

// Upload a blocchi: Cloudflare limita una richiesta a 100 MB, il pannello ricompone i pezzi.
import { api } from './api';

type UploadInfo = { id: string; chunk_size: number; chunks: number };
type Status = { received: number[]; complete: boolean };

export async function uploadFile(siteId: number, path: string, file: File, onProgress: (fraction: number) => void) {
	return uploadTo(`/api/sites/${siteId}/files/uploads`, path, file, onProgress);
}

/** Stesso protocollo su un'altra area (es. `/api/imports/uploads`). */
export async function uploadTo(base: string, path: string, file: File, onProgress: (fraction: number) => void) {
	const info = await api.post<UploadInfo>(base, { path, size: file.size });
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

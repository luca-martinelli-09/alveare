// Client dell'API del pannello: stessa origine, cookie di sessione e header anti-CSRF.
import { goto } from '$app/navigation';

export class ApiError extends Error {
	constructor(
		public status: number,
		message: string
	) {
		super(message);
	}
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
	const headers: Record<string, string> = { 'x-alveare-csrf': '1' };
	let payload: BodyInit | undefined;
	if (body instanceof Blob || body instanceof ArrayBuffer || typeof body === 'string') {
		payload = body as BodyInit;
	} else if (body !== undefined) {
		headers['content-type'] = 'application/json';
		payload = JSON.stringify(body);
	}
	const res = await fetch(path, { method, headers, body: payload, credentials: 'same-origin' });
	if (res.status === 401 && !path.startsWith('/api/auth/') && path !== '/api/me') {
		await goto('/login');
	}
	const text = await res.text();
	const data = text ? JSON.parse(text) : null;
	if (!res.ok) throw new ApiError(res.status, data?.error ?? res.statusText);
	return data as T;
}

export const api = {
	get: <T>(path: string) => request<T>('GET', path),
	post: <T>(path: string, body?: unknown) => request<T>('POST', path, body),
	put: <T>(path: string, body?: unknown) => request<T>('PUT', path, body),
	patch: <T>(path: string, body?: unknown) => request<T>('PATCH', path, body),
	del: <T>(path: string) => request<T>('DELETE', path)
};

export type Stage = 'setup' | 'totp' | 'full';
export type User = { id: number; email: string; name: string; is_admin: boolean; status?: 'invited' | 'active' | 'blocked' };
export type Monitor = {
	uptime: { ok?: boolean; status?: number; ms?: number; at?: number; error?: string | null } | null;
	usage: { files_bytes?: number; db_bytes?: number; at?: number } | null;
	traffic: { requests?: number; errors_5xx?: number; slow?: number } | null;
};
export type SiteRow = {
	id: number;
	slug: string;
	domain: string | null;
	kind: string;
	status: string;
	online: boolean | null;
	staging: boolean;
	monitor: Monitor;
	last_backup: { started_at: string; status: string } | null;
	/** Backup configurati sul server (RESTIC_REPOSITORY). */
	backups: boolean;
	url: string | null;
	/** Soglia d'avviso dello spazio (file + database). */
	disk_alert_mb: number;
};
export type Job = { id: number; kind: string; site_id: number | null; status: 'running' | 'done' | 'failed'; log: string[] };
export type Alert = { id: number; site_id: number | null; kind: string; message: string; created_at: string };

/** Attende la fine di un job aggiornando il log man mano. */
export async function waitJob(id: number, onUpdate: (job: Job) => void): Promise<Job> {
	for (;;) {
		const job = await api.get<Job>(`/api/jobs/${id}`);
		onUpdate(job);
		if (job.status !== 'running') return job;
		await new Promise((r) => setTimeout(r, 1000));
	}
}

export function formatBytes(n: number | null | undefined): string {
	if (n == null) return '–';
	const units = ['B', 'KB', 'MB', 'GB', 'TB'];
	let i = 0;
	let v = n;
	while (v >= 1024 && i < units.length - 1) {
		v /= 1024;
		i++;
	}
	return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export function errorMessage(e: unknown): string {
	return e instanceof Error ? e.message : String(e);
}

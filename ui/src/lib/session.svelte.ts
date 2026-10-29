import { api, type Stage, type User } from './api';

export const session = $state<{ user: User | null; stage: Stage | null; loaded: boolean }>({
	user: null,
	stage: null,
	loaded: false
});

export async function loadSession() {
	try {
		const me = await api.get<{ user: User; stage: Stage }>('/api/me');
		session.user = me.user;
		session.stage = me.stage;
	} catch {
		session.user = null;
		session.stage = null;
	}
	session.loaded = true;
}

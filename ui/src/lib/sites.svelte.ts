// Elenco dei siti condiviso da barra laterale e panoramica.
import { api, type SiteRow } from './api';

export const sites = $state<{ list: SiteRow[] | null }>({ list: null });

export async function loadSites() {
	sites.list = await api.get<SiteRow[]>('/api/sites');
}

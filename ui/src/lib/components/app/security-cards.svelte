<script lang="ts">
	import { askConfirm, askPassword, askText } from '$lib/ask.svelte';
	// Profilo: passkey, sessioni aperte e token API, con revoca.
	import SettingsSection from './settings-section.svelte';
	import SettingRow from './setting-row.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { api, errorMessage } from '$lib/api';
	import { createPasskey, passkeysSupported } from '$lib/passkey';
	import { toast } from 'svelte-sonner';

	let { refresh = 0, part = 'all' }: { refresh?: number; part?: 'all' | 'security' | 'tokens' } = $props();
	type Passkey = { id: number; name: string; created_at: number; last_used: number | null };
	type Sess = { id: string; created_at: number; last_used: number; ip: string; user_agent: string; current: boolean };
	type Tok = { id: number; name: string; expires_at: number; last_used: number | null };
	let passkeys = $state<{ enabled: boolean; passkeys: Passkey[] }>({ enabled: false, passkeys: [] });
	let sessions = $state<Sess[]>([]);
	let tokens = $state<Tok[]>([]);

	async function load() {
		[passkeys, sessions, tokens] = await Promise.all([api.get<typeof passkeys>('/api/me/passkeys'), api.get<Sess[]>('/api/me/sessions'), api.get<Tok[]>('/api/me/tokens')]);
	}

	$effect(() => {
		refresh;
		load();
	});

	async function act(fn: () => Promise<unknown>, done?: string) {
		try {
			await fn();
			if (done) toast.success(done);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const addPasskey = () =>
		act(async () => {
			const name = await askText({ title: 'Nuova passkey', description: 'Poi il browser chiede impronta, volto o PIN.', label: 'Nome del dispositivo', value: 'Portatile', confirmLabel: 'Continua' });
			if (!name) return;
			const password = await askPassword('Conferma la password');
			if (!password) return;
			const start = await api.post<{ id: string; options: unknown }>('/api/me/passkeys/register/start', { password });
			const credential = await createPasskey(start.options);
			await api.post('/api/me/passkeys/register/finish', { id: start.id, name, credential });
		}, 'Passkey aggiunta');

	// "Firefox su Linux" invece dell'user agent intero.
	function device(ua: string) {
		if (!ua) return 'Dispositivo sconosciuto';
		const browser = /Edg\//.test(ua) ? 'Edge' : /Firefox\//.test(ua) ? 'Firefox' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : ua.split(/[\/ ]/)[0];
		const os = /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Mac OS X/.test(ua) ? 'macOS' : /Windows/.test(ua) ? 'Windows' : /Linux/.test(ua) ? 'Linux' : '';
		return os ? `${browser} su ${os}` : browser;
	}

	const when = (t: number | null) => (t ? new Date(t * 1000).toLocaleString('it-IT', { dateStyle: 'short', timeStyle: 'short' }) : '–');
</script>

{#if part !== 'tokens'}
<SettingsSection title="Passkey" description="Accesso senza password: impronta, volto o PIN del dispositivo valgono come secondo fattore.">
	{#each passkeys.passkeys as p (p.id)}
		<SettingRow label={p.name} help={`Usata ${when(p.last_used)}`}>
			<Button size="sm" onclick={async () => (await askConfirm({ title: `Togliere la passkey ${p.name}?`, description: 'Da quel dispositivo servirà di nuovo password e codice.', confirmLabel: 'Togli', destructive: true })) && act(() => api.del(`/api/me/passkeys/${p.id}`))}>Togli</Button>
		</SettingRow>
	{/each}
	<SettingRow
		label={passkeys.passkeys.length ? 'Aggiungi un altro dispositivo' : 'Nessuna passkey'}
		help={!passkeys.enabled ? 'Le passkey richiedono ALVEARE_PANEL_URL.' : !passkeysSupported() ? 'Questo browser non supporta le passkey.' : 'Un dispositivo alla volta: telefono, portatile, chiave di sicurezza.'}
	>
		<Button size="sm" disabled={!passkeys.enabled || !passkeysSupported()} onclick={addPasskey}>Aggiungi una passkey</Button>
	</SettingRow>
</SettingsSection>

<SettingsSection title="Sessioni aperte" description="Scadono dopo 12 ore senza attività.">
	{#each sessions as s (s.id)}
		<SettingRow help={`${s.ip} · ultima attività ${when(s.last_used)}`}>
			{#snippet label()}<p class="truncate font-medium" title={s.user_agent}>{device(s.user_agent)}</p>{/snippet}
			{#if s.current}<Badge variant="secondary">questa sessione</Badge>
			{:else}<Button size="sm" onclick={() => act(() => api.del(`/api/me/sessions/${s.id}`), 'Sessione chiusa')}>Chiudi</Button>{/if}
		</SettingRow>
	{/each}
	{#if sessions.length > 1}
		<div class="flex justify-end px-4 py-2.5">
			<Button variant="ghost" size="sm" onclick={() => act(() => api.post('/api/me/sessions/others'), 'Altre sessioni chiuse')}>Esci dagli altri dispositivi</Button>
		</div>
	{/if}
</SettingsSection>

{/if}

{#if part !== 'security' && tokens.length}
	<SettingsSection title="I tuoi token API">
		{#each tokens as t (t.id)}
			<SettingRow label={t.name} help={`Scade ${when(t.expires_at)} · usato ${when(t.last_used)}`}>
				<Button size="sm" onclick={async () => (await askConfirm({ title: `Revocare il token ${t.name}?`, description: 'Gli script che lo usano smettono subito di funzionare.', confirmLabel: 'Revoca', destructive: true })) && act(() => api.del(`/api/me/tokens/${t.id}`), 'Token revocato')}>Revoca</Button>
			</SettingRow>
		{/each}
	</SettingsSection>
{/if}

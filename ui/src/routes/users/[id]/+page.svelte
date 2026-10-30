<script lang="ts">
	// Un utente visto dagli admin: profilo e ruolo, permessi per sito, sicurezza (password, TOTP, sessioni).
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import * as Breadcrumb from '$lib/components/ui/breadcrumb';
	import * as Tabs from '$lib/components/ui/tabs';
	import * as Select from '$lib/components/ui/select';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import SiteAvatar from '$lib/components/app/site-avatar.svelte';
	import { api, errorMessage, type User } from '$lib/api';
	import { askConfirm } from '$lib/ask.svelte';
	import { session } from '$lib/session.svelte';
	import { sites } from '$lib/sites.svelte';
	import { toast } from 'svelte-sonner';

	type Sess = { id: string; last_used: number; ip: string; user_agent: string };
	type Detail = { user: User; status: string; has_totp: boolean; must_change_password: boolean; passkeys: number; sessions: Sess[] };
	type Assignment = { site_id: number; role: 'manager' | 'readonly' };

	const id = $derived(Number(page.params.id));
	let d = $state<Detail | null>(null);
	let assignments = $state<Assignment[]>([]);
	let name = $state('');
	let email = $state('');
	let isAdmin = $state(false);
	let temp = $state<string | null>(null);
	const self = $derived(session.user?.id === id);

	async function load() {
		[d, assignments] = await Promise.all([api.get<Detail>(`/api/users/${id}`), api.get<Assignment[]>(`/api/users/${id}/sites`)]);
		name = d.user.name;
		email = d.user.email;
		isAdmin = d.user.is_admin;
	}
	$effect(() => {
		load();
	});

	const tab = $derived(page.url.searchParams.get('tab') ?? 'profile');
	function setTab(v: string) {
		const url = new URL(page.url.href);
		if (v === 'profile') url.searchParams.delete('tab');
		else url.searchParams.set('tab', v);
		goto(url, { replace: true, reset: false });
	}

	const dirty = $derived(!!d && (name.trim() !== d.user.name || email.trim() !== d.user.email || isAdmin !== d.user.is_admin));

	async function run(fn: () => Promise<unknown>, done: string) {
		try {
			await fn();
			toast.success(done);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const save = () => run(() => api.patch(`/api/users/${id}`, { name, email, is_admin: isAdmin }), 'Utente aggiornato');

	async function block(blocked: boolean) {
		if (blocked && !(await askConfirm({ title: `Bloccare ${d?.user.name}?`, description: 'Non può più accedere e le sue sessioni si chiudono subito.', confirmLabel: 'Blocca', destructive: true }))) return;
		run(() => api.post(`/api/users/${id}/block`, { blocked }), blocked ? 'Utente bloccato' : 'Utente sbloccato');
	}

	async function resetPassword() {
		if (!(await askConfirm({ title: 'Generare una password temporanea?', description: 'Quella attuale smette di valere; sessioni, token API e passkey vengono revocati. Al primo accesso la cambierà.', confirmLabel: 'Genera' }))) return;
		try {
			temp = (await api.post<{ temporary_password: string }>(`/api/users/${id}/password`)).temporary_password;
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function resetTotp() {
		if (!(await askConfirm({ title: 'Azzerare il TOTP?', description: 'Per un telefono perso: al prossimo accesso sceglierà una nuova password e configurerà di nuovo l\'app. Sessioni, token API e passkey vengono revocati.', confirmLabel: 'Azzera', destructive: true }))) return;
		run(() => api.post(`/api/users/${id}/totp/reset`), 'TOTP azzerato');
	}

	const roleOf = (siteId: number) => assignments.find((a) => a.site_id === siteId)?.role ?? 'none';
	const roles: Record<string, string> = { none: 'Nessun accesso', manager: 'Gestore', readonly: 'Sola lettura' };
	async function setRole(siteId: number, role: string) {
		try {
			if (role === 'none') await api.del(`/api/users/${id}/sites/${siteId}`);
			else await api.put(`/api/users/${id}/sites/${siteId}`, { role });
			assignments = await api.get<Assignment[]>(`/api/users/${id}/sites`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	function device(ua: string) {
		if (!ua) return 'Dispositivo sconosciuto';
		const browser = /Edg\//.test(ua) ? 'Edge' : /Firefox\//.test(ua) ? 'Firefox' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : ua.split(/[/ ]/)[0];
		const os = /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Mac OS X/.test(ua) ? 'macOS' : /Windows/.test(ua) ? 'Windows' : /Linux/.test(ua) ? 'Linux' : '';
		return os ? `${browser} su ${os}` : browser;
	}
	const when = (t: number) => new Date(t * 1000).toLocaleString('it-IT', { dateStyle: 'short', timeStyle: 'short' });
	const statuses: Record<string, string> = { active: 'attivo', invited: 'invitato', blocked: 'bloccato' };
</script>

<Breadcrumb.Root>
	<Breadcrumb.List>
		<Breadcrumb.Item><Breadcrumb.Link href="/users">Utenti</Breadcrumb.Link></Breadcrumb.Item>
		<Breadcrumb.Separator />
		<Breadcrumb.Item><Breadcrumb.Page>{d?.user.name ?? '…'}</Breadcrumb.Page></Breadcrumb.Item>
	</Breadcrumb.List>
</Breadcrumb.Root>

{#if !d}
	<Skeleton class="h-48 w-full" />
{:else}
	<header class="-mt-2 flex flex-col gap-1.5">
		<h1 class="flex flex-wrap items-center gap-2 text-2xl font-semibold tracking-tight">
			{d.user.name}
			{#if d.user.is_admin}<Badge>admin</Badge>{/if}
			{#if d.status !== 'active'}<Badge variant={d.status === 'blocked' ? 'destructive' : 'secondary'}>{statuses[d.status] ?? d.status}</Badge>{/if}
		</h1>
		<p class="text-muted-foreground text-sm">{d.user.email}{self ? ' · sei tu' : ''}</p>
	</header>

	<Tabs.Root value={tab} onValueChange={setTab} class="gap-0">
		<div class="-mx-4 overflow-x-auto overflow-y-hidden border-b px-4 [scrollbar-width:none] md:mx-0 md:px-0">
			<Tabs.List variant="line" class="h-10">
				<Tabs.Trigger value="profile">Profilo</Tabs.Trigger>
				{#if !d.user.is_admin}<Tabs.Trigger value="sites">Siti</Tabs.Trigger>{/if}
				<Tabs.Trigger value="security">Sicurezza</Tabs.Trigger>
			</Tabs.List>
		</div>

		<Tabs.Content value="profile" class="flex flex-col gap-6 pt-6">
			<SettingsSection title="Dati e ruolo">
				<SettingRow label="Nome" labelFor="u-name">
					<Input id="u-name" class="w-64" bind:value={name} />
				</SettingRow>
				<SettingRow label="Email" labelFor="u-email" help="Per accedere e per gli inviti.">
					<Input id="u-email" type="email" class="w-64" bind:value={email} />
				</SettingRow>
				<SettingRow label="Amministratore" labelFor="u-admin" help="Vede e gestisce tutti i siti, gli utenti e il server.">
					<Switch id="u-admin" bind:checked={isAdmin} disabled={self && d.user.is_admin} />
				</SettingRow>
				{#if dirty}
					<div class="bg-muted/50 flex flex-wrap items-center justify-end gap-2 rounded-b-lg px-4 py-2.5">
						<span class="text-muted-foreground mr-auto text-sm">Modifiche non salvate</span>
						<Button variant="ghost" size="sm" onclick={() => d && ((name = d.user.name), (email = d.user.email), (isAdmin = d.user.is_admin))}>Annulla</Button>
						<Button variant="primary" size="sm" onclick={save}>Salva modifiche</Button>
					</div>
				{/if}
			</SettingsSection>
			{#if !self}
				<SettingsSection title="Accesso">
					<SettingRow
						label={d.status === 'blocked' ? 'Utente bloccato' : 'Blocca l\'utente'}
						help={d.status === 'blocked' ? 'Non può accedere finché non lo sblocchi.' : 'Non potrà più accedere; le sue sessioni si chiudono subito.'}
					>
						{#if d.status === 'blocked'}<Button size="sm" onclick={() => block(false)}>Sblocca</Button>
						{:else}<Button variant="destructive" size="sm" onclick={() => block(true)}>Blocca…</Button>{/if}
					</SettingRow>
				</SettingsSection>
			{/if}
		</Tabs.Content>

		{#if !d.user.is_admin}
			<Tabs.Content value="sites" class="flex flex-col gap-6 pt-6">
				<SettingsSection title="Permessi per sito" description="Il ruolo vale per quel sito: gestore su uno, sola lettura su un altro.">
					{#each sites.list ?? [] as s (s.id)}
						<SettingRow>
							{#snippet label()}
								<span class="flex items-center gap-2.5 font-medium"><SiteAvatar name={s.domain ?? s.slug} />{s.domain ?? s.slug}</span>
							{/snippet}
							<Select.Root type="single" value={roleOf(s.id)} onValueChange={(v) => setRole(s.id, v)}>
								<Select.Trigger class="w-44" aria-label={`Ruolo su ${s.domain ?? s.slug}`}>{roles[roleOf(s.id)]}</Select.Trigger>
								<Select.Content>
									<Select.Group>
										{#each Object.entries(roles) as [value, label] (value)}
											<Select.Item {value} {label}>{label}</Select.Item>
										{/each}
									</Select.Group>
								</Select.Content>
							</Select.Root>
						</SettingRow>
					{:else}
						<SettingRow label="Nessun sito" help="Crea un sito per assegnarlo." />
					{/each}
				</SettingsSection>
			</Tabs.Content>
		{/if}

		<Tabs.Content value="security" class="flex flex-col gap-6 pt-6">
			<SettingsSection title="Credenziali">
				{#if temp}
					<div class="px-4 py-3">
						<Alert.Root>
							<Alert.Title>Password temporanea per {d.user.email}</Alert.Title>
							<Alert.Description>
								<code class="text-base select-all">{temp}</code>
								<p>Comunicala di persona o con un canale sicuro: non verrà più mostrata.</p>
							</Alert.Description>
						</Alert.Root>
					</div>
				{/if}
				<SettingRow label="Password" help={d.must_change_password ? 'Deve cambiarla al prossimo accesso.' : 'Se l\'ha dimenticata e il reset via email non è attivo.'}>
					<Button size="sm" onclick={resetPassword}>Password temporanea…</Button>
				</SettingRow>
				<SettingRow
					label="App di autenticazione"
					help={d.has_totp ? 'Configurata. Azzerala se ha perso il telefono.' : 'Non configurata: la imposterà al prossimo accesso.'}
				>
					{#if d.has_totp}<Button variant="destructive" size="sm" onclick={resetTotp}>Azzera…</Button>{/if}
				</SettingRow>
				<SettingRow label="Passkey" help={d.passkeys ? `${d.passkeys} ${d.passkeys === 1 ? 'dispositivo registrato' : 'dispositivi registrati'}; le gestisce dal suo account.` : 'Nessuna passkey.'} />
			</SettingsSection>
			<SettingsSection title="Sessioni aperte">
				{#each d.sessions as s (s.id)}
					<SettingRow help={`${s.ip} · ultima attività ${when(s.last_used)}`}>
						{#snippet label()}<p class="truncate font-medium" title={s.user_agent}>{device(s.user_agent)}</p>{/snippet}
						<Button size="sm" onclick={() => run(() => api.del(`/api/users/${id}/sessions/${s.id}`), 'Sessione chiusa')}>Chiudi</Button>
					</SettingRow>
				{:else}
					<SettingRow label="Nessuna sessione aperta" />
				{/each}
				{#if d.sessions.length}
					<div class="flex justify-end px-4 py-2.5">
						<Button variant="ghost" size="sm" onclick={async () => (await askConfirm({ title: 'Chiudere tutto?', description: 'Si chiudono tutte le sessioni e smettono di valere anche i token API.', confirmLabel: 'Chiudi tutto', destructive: true })) && run(() => api.post(`/api/users/${id}/sessions/revoke`), 'Sessioni chiuse e token revocati')}>Chiudi tutto</Button>
					</div>
				{/if}
			</SettingsSection>
		</Tabs.Content>
	</Tabs.Root>
{/if}

<script lang="ts">
	// Account: profilo, sicurezza (TOTP, passkey, sessioni), token API e kit di ripristino (admin), a schede.
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import PageHeader from '$lib/components/app/page-header.svelte';
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import SecurityCards from '$lib/components/app/security-cards.svelte';
	import PasswordDialog from '$lib/components/app/password-dialog.svelte';
	import TotpDialog from '$lib/components/app/totp-dialog.svelte';
	import * as Tabs from '$lib/components/ui/tabs';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, errorMessage } from '$lib/api';
	import { session, loadSession } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import DownloadIcon from '@lucide/svelte/icons/download';

	const tab = $derived(page.url.searchParams.get('tab') ?? 'profile');
	function setTab(v: string) {
		const url = new URL(page.url.href);
		if (v === 'profile') url.searchParams.delete('tab');
		else url.searchParams.set('tab', v);
		goto(url, { replace: true, reset: false });
	}

	let name = $state('');
	let email = $state('');
	$effect(() => {
		name = session.user?.name ?? '';
		email = session.user?.email ?? '';
	});
	const dirty = $derived(name.trim() !== (session.user?.name ?? '') || email.trim() !== (session.user?.email ?? ''));
	let totpOn = $state(true);
	$effect(() => {
		api.get<{ totp: boolean }>('/api/auth/options').then((o) => (totpOn = o.totp));
	});
	let changingPassword = $state(false);
	let changingTotp = $state(false);

	async function saveProfile() {
		try {
			await api.patch('/api/me', { name, email });
			await loadSession();
			toast.success('Profilo aggiornato');
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	let tokenName = $state('');
	let days = $state(90);
	let token = $state<string | null>(null);
	let refresh = $state(0);

	async function createToken(e: SubmitEvent) {
		e.preventDefault();
		try {
			token = (await api.post<{ token: string }>('/api/me/tokens', { name: tokenName, days: Number(days) })).token;
			tokenName = '';
			refresh++;
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}
</script>

<PageHeader title="Account" description={session.user ? `${session.user.name} · ${session.user.email}` : undefined} />

<Tabs.Root value={tab} onValueChange={setTab} class="gap-0">
	<div class="-mx-4 overflow-x-auto overflow-y-hidden border-b px-4 [scrollbar-width:none] md:mx-0 md:px-0">
		<Tabs.List variant="line" class="h-10">
			<Tabs.Trigger value="profile">Profilo</Tabs.Trigger>
			<Tabs.Trigger value="security">Sicurezza</Tabs.Trigger>
			<Tabs.Trigger value="tokens">Token API</Tabs.Trigger>
			{#if session.user?.is_admin}<Tabs.Trigger value="recovery">Ripristino</Tabs.Trigger>{/if}
		</Tabs.List>
	</div>

	<Tabs.Content value="profile" class="flex flex-col gap-6 pt-6">
		<SettingsSection title="Dati">
			<SettingRow label="Nome" labelFor="me-name" help="Come ti vedono gli altri nell'attività.">
				<Input id="me-name" class="w-64" bind:value={name} />
			</SettingRow>
			<SettingRow label="Email" labelFor="me-email" help="Per accedere, per gli inviti e per il reset della password.">
				<Input id="me-email" type="email" class="w-64" bind:value={email} />
			</SettingRow>
			{#if dirty}
				<div class="bg-muted/50 flex flex-wrap items-center justify-end gap-2 rounded-b-lg px-4 py-2.5">
					<span class="text-muted-foreground mr-auto text-sm">Modifiche non salvate</span>
					<Button variant="ghost" size="sm" onclick={() => ((name = session.user?.name ?? ''), (email = session.user?.email ?? ''))}>Annulla</Button>
					<Button variant="primary" size="sm" onclick={saveProfile}>Salva modifiche</Button>
				</div>
			{/if}
		</SettingsSection>
		<SettingsSection title="Password">
			<SettingRow label="Password" help="Almeno 12 caratteri. Cambiandola si chiudono le sessioni sugli altri dispositivi.">
				<Button size="sm" onclick={() => (changingPassword = true)}>Cambia password</Button>
			</SettingRow>
		</SettingsSection>
	</Tabs.Content>

	<Tabs.Content value="security" class="flex flex-col gap-6 pt-6">
		{#if totpOn}
			<SettingsSection title="Secondo fattore">
				<SettingRow help="Il codice a sei cifre chiesto dopo la password. Se cambi telefono, sposta qui il TOTP sulla nuova app.">
					{#snippet label()}<p class="flex items-center gap-2 font-medium">App di autenticazione <Badge variant="secondary">attiva</Badge></p>{/snippet}
					<Button size="sm" onclick={() => (changingTotp = true)}>Sposta su un'altra app</Button>
				</SettingRow>
			</SettingsSection>
		{/if}
		<SecurityCards {refresh} part="security" />
	</Tabs.Content>

	<Tabs.Content value="tokens" class="flex flex-col gap-6 pt-6">
		<SettingsSection title="Nuovo token" description="Per script e per la CLI alveare: hanno i tuoi stessi permessi.">
			{#if token}
				<div class="px-4 py-3">
					<Alert.Root>
						<Alert.Title>Copia il token adesso: non verrà più mostrato</Alert.Title>
						<Alert.Description><code class="break-all select-all">{token}</code></Alert.Description>
					</Alert.Root>
				</div>
			{/if}
			<SettingRow label="Nome e durata" labelFor="tname" help="Un nome per riconoscerlo e quanti giorni vale.">
				<form class="flex flex-wrap items-center gap-2" onsubmit={createToken}>
					<Input id="tname" class="w-52" placeholder="backup del portatile" bind:value={tokenName} required />
					<Input id="days" type="number" min={1} max={365} class="w-20 text-right" aria-label="Validità in giorni" bind:value={days} />
					<span class="text-muted-foreground text-sm">giorni</span>
					<Button type="submit" size="sm">Crea token</Button>
				</form>
			</SettingRow>
		</SettingsSection>
		<SecurityCards {refresh} part="tokens" />
	</Tabs.Content>

	{#if session.user?.is_admin}
		<Tabs.Content value="recovery" class="flex flex-col gap-6 pt-6">
			<SettingsSection title="Kit di ripristino">
				<SettingRow label="Chiavi e istruzioni per ricostruire il server" help="Conservalo fuori dal server, per esempio nel password manager; riscaricalo quando una chiave cambia.">
					<Button href="/api/admin/recovery-kit" size="sm"><DownloadIcon data-icon="inline-start" /> Scarica il kit</Button>
				</SettingRow>
			</SettingsSection>
		</Tabs.Content>
	{/if}
</Tabs.Root>

<PasswordDialog bind:open={changingPassword} />
<TotpDialog bind:open={changingTotp} />

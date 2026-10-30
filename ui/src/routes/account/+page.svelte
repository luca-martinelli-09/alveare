<script lang="ts">
	import PageHeader from '$lib/components/app/page-header.svelte';
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import SecurityCards from '$lib/components/app/security-cards.svelte';

	let name = $state('');
	let days = $state(90);
	let token = $state<string | null>(null);
	let refresh = $state(0);

	async function createToken(e: SubmitEvent) {
		e.preventDefault();
		try {
			token = (await api.post<{ token: string }>('/api/me/tokens', { name, days: Number(days) })).token;
			name = '';
			refresh++;
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}
</script>

<PageHeader title="Account" description="Password, secondo fattore, passkey, sessioni e token API." />
<SettingsSection title="Token API" description="Per script e per la CLI alveare: hanno i tuoi stessi permessi.">
	{#if token}
		<div class="px-4 py-3">
			<Alert.Root>
				<Alert.Title>Copia il token adesso: non verrà più mostrato</Alert.Title>
				<Alert.Description><code class="break-all select-all">{token}</code></Alert.Description>
			</Alert.Root>
		</div>
	{/if}
	<SettingRow label="Nuovo token" labelFor="tname" help="Un nome per riconoscerlo e quanti giorni vale.">
		<form class="flex flex-wrap items-center gap-2" onsubmit={createToken}>
			<Input id="tname" class="w-52" placeholder="backup del portatile" bind:value={name} required />
			<Input id="days" type="number" min={1} max={365} class="w-20 text-right" aria-label="Validità in giorni" bind:value={days} />
			<span class="text-muted-foreground text-sm">giorni</span>
			<Button type="submit" size="sm">Crea token</Button>
		</form>
	</SettingRow>
</SettingsSection>
<SecurityCards {refresh} />
{#if session.user?.is_admin}
	<SettingsSection title="Kit di ripristino">
		<SettingRow label="Chiavi e istruzioni per ricostruire il server" help="Conservalo fuori dal server, per esempio nel password manager; riscaricalo quando una chiave cambia.">
			<Button href="/api/admin/recovery-kit" size="sm"><DownloadIcon data-icon="inline-start" /> Scarica il kit</Button>
		</SettingRow>
	</SettingsSection>
{/if}

<script lang="ts">
	// Mittente delle email del sito (SMTP_FROM e SMTP_FROM_NAME nel suo .env) e invio di prova.
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Spinner } from '$lib/components/ui/spinner';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';

	let { id }: { id: number } = $props();

	let saved = $state<{ from: string; from_name: string; host: string } | null>(null);
	let from = $state('');
	let fromName = $state('');
	let to = $state('');
	let sending = $state(false);
	let result = $state<{ ok: boolean; error: string | null } | null>(null);

	async function load() {
		try {
			saved = await api.get(`/api/sites/${id}/email`);
			[from, fromName] = [saved!.from, saved!.from_name];
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
	$effect(() => {
		load();
		to = session.user?.email ?? '';
	});
	const dirty = $derived(saved !== null && (from.trim() !== saved.from || fromName.trim() !== saved.from_name));

	async function save() {
		try {
			await api.put(`/api/sites/${id}/email`, { from, from_name: fromName });
			toast.success('Mittente salvato');
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function sendTest() {
		sending = true;
		result = null;
		try {
			result = await api.post(`/api/sites/${id}/email/test`, { to });
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			sending = false;
		}
	}
</script>

<SettingsSection title="Email" description={saved?.host ? `Le email di WordPress partono da ${saved.host}. Il mittente deve essere verificato lì (per SES: il dominio con DKIM).` : "SMTP non configurato nel .env del server (SITE_ENV_SMTP_*): le email non partono."}>
	<SettingRow label="Mittente" labelFor="smtp-from" help="Indirizzo e nome che vedono i destinatari.">
		<div class="flex w-full flex-wrap gap-2 sm:w-auto">
			<Input id="smtp-from" type="email" class="sm:w-60" bind:value={from} placeholder="info@esempio.it" />
			<Input aria-label="Nome del mittente" class="sm:w-48" bind:value={fromName} placeholder="Nome" />
			{#if dirty}<Button onclick={save}>Salva</Button>{/if}
		</div>
	</SettingRow>
	<SettingRow label="Email di prova" labelFor="smtp-test" help="La manda WordPress con le impostazioni attuali; se fallisce vedi l'errore del server di posta.">
		{#snippet extra()}
			{#if result}
				<Alert.Root variant={result.ok ? 'default' : 'destructive'} class="mt-3">
					<Alert.Title>{result.ok ? `Inviata a ${to}` : 'Non inviata'}</Alert.Title>
					<Alert.Description>{result.ok ? 'Controlla la casella (anche lo spam).' : result.error || 'Nessun dettaglio dal server di posta.'}</Alert.Description>
				</Alert.Root>
			{/if}
		{/snippet}
		<div class="flex w-full flex-wrap gap-2 sm:w-auto">
			<Input id="smtp-test" type="email" class="sm:w-60" bind:value={to} />
			<Button onclick={sendTest} disabled={sending || dirty || !to.trim()} title={dirty ? 'Salva prima il mittente' : undefined}>
				{#if sending}<Spinner data-icon="inline-start" />{/if}
				Invia prova
			</Button>
		</div>
	</SettingRow>
</SettingsSection>

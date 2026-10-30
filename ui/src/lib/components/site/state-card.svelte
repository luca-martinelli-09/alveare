<script lang="ts">
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Switch } from '$lib/components/ui/switch';
	import { Input } from '$lib/components/ui/input';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import type { SiteDetail } from './types';

	let { detail, onchange }: { detail: SiteDetail; onchange: () => void } = $props();
	let busy = $state(false);
	const suspended = $derived(detail.site.status === 'suspended');
	let logDays = $state(0);
	$effect(() => {
		logDays = detail.site.log_retention_days;
	});

	async function update(body: Record<string, boolean | number>, done: string) {
		busy = true;
		try {
			await api.patch(`/api/sites/${detail.site.id}`, body);
			toast.success(done);
			onchange();
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			busy = false;
		}
	}
</script>

<SettingsSection title="Stato e accesso">
	<SettingRow label="Sito sospeso" help="I visitatori vedono una pagina di manutenzione e PHP si ferma; file e database restano intatti.">
		{#if suspended}
			<Button disabled={busy} onclick={() => update({ suspended: false }, 'Sito riattivato')}>Riattiva il sito</Button>
		{:else}
			<AlertDialog.Root>
				<AlertDialog.Trigger>
					{#snippet child({ props })}
						<Button variant="outline" size="sm" disabled={busy} {...props}>Sospendi…</Button>
					{/snippet}
				</AlertDialog.Trigger>
				<AlertDialog.Content>
					<AlertDialog.Header>
						<AlertDialog.Title>Sospendere {detail.domains[0]?.hostname}?</AlertDialog.Title>
						<AlertDialog.Description>
							I visitatori vedono una pagina di manutenzione e PHP si ferma. File e database restano intatti.
						</AlertDialog.Description>
					</AlertDialog.Header>
					<AlertDialog.Footer>
						<AlertDialog.Cancel>Annulla</AlertDialog.Cancel>
						<AlertDialog.Action onclick={() => update({ suspended: true }, 'Sito sospeso')}>Sospendi</AlertDialog.Action>
					</AlertDialog.Footer>
				</AlertDialog.Content>
			</AlertDialog.Root>
		{/if}
	</SettingRow>
	<SettingRow label="Password sulle copie di prova" labelFor="staging" help="Chiede una password ai visitatori e blocca l'indicizzazione nei motori di ricerca.">
		{#snippet extra()}
			{#if detail.staging}
				<p class="mt-1 text-sm">Utente <code>{detail.staging.user}</code>, password <code class="select-all">{detail.staging.password}</code></p>
			{/if}
		{/snippet}
		<Switch
			id="staging"
			checked={detail.site.staging}
			disabled={busy || suspended}
			onCheckedChange={(on) => update({ staging: on }, on ? 'Protezione attivata' : 'Protezione tolta')}
		/>
	</SettingRow>
	<SettingRow label="Conservazione dei log" labelFor="log-days" help="Gli access log contengono IP: dati personali. Dopo questi giorni vengono eliminati.">
		<Input
			id="log-days"
			type="number"
			min="1"
			max="365"
			class={['w-20 text-right tabular-nums', logDays !== detail.site.log_retention_days && 'border-primary bg-accent']}
			bind:value={logDays}
		/>
		<span class="text-muted-foreground w-10 text-sm">giorni</span>
	</SettingRow>
	{#if logDays !== detail.site.log_retention_days}
		<div class="bg-muted/50 flex flex-wrap items-center justify-end gap-2 rounded-b-lg px-4 py-2.5">
			<span class="text-muted-foreground mr-auto text-sm">1 modifica: conservazione dei log {detail.site.log_retention_days} → {logDays} giorni</span>
			<Button variant="ghost" size="sm" onclick={() => (logDays = detail.site.log_retention_days)}>Annulla</Button>
			<Button variant="primary" size="sm" disabled={busy} onclick={() => update({ log_retention_days: logDays }, 'Conservazione aggiornata')}>Salva modifiche</Button>
		</div>
	{/if}
</SettingsSection>

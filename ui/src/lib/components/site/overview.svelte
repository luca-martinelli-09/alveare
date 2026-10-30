<script lang="ts">
	import { goto } from '$app/navigation';
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import DomainsCard from './domains-card.svelte';
	import StateCard from './state-card.svelte';
	import CopiesCard from './copies-card.svelte';
	import MonitorCard from './monitor-card.svelte';
	import type { SiteDetail } from './types';

	let { detail, onchange }: { detail: SiteDetail; onchange: () => void } = $props();
	let deleting = $state<number | null>(null);

	let wpAdmin = $state<{ user: string; password: string } | null>(null);

	async function revealWpAdmin() {
		try {
			wpAdmin = await api.post<{ user: string; password: string }>(`/api/sites/${detail.site.id}/wp-admin`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function remove() {
		try {
			deleting = (await api.del<{ job: number }>(`/api/sites/${detail.site.id}`)).job;
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
</script>

<div class="flex flex-col gap-6 pt-6">
	{#if detail.wp_admin_pending}
		<SettingsSection title="Accesso a WordPress">
			<SettingRow label="Password dell'utente admin" help={'Si mostra una volta sola, entro 24 ore dalla creazione. Poi c\'è "Password dimenticata" di WordPress.'}>
				{#snippet extra()}
					{#if wpAdmin}<p class="mt-1 text-sm">Utente <code>{wpAdmin.user}</code>, password <code class="select-all">{wpAdmin.password}</code></p>{/if}
				{/snippet}
				{#if !wpAdmin}<Button onclick={revealWpAdmin}>Mostra la password</Button>{/if}
			</SettingRow>
		</SettingsSection>
	{/if}
	<MonitorCard id={detail.site.id} />
	<DomainsCard {detail} {onchange} />
	{#if detail.can_manage}
		<StateCard {detail} {onchange} />
		<CopiesCard {detail} />
	{/if}
	<SettingsSection title="PHP" description={`Pool dedicato con l'utente Unix ${detail.site.slug}.`}>
		<SettingRow label={`PHP ${detail.site.php_version}`} help={`memory_limit ${detail.site.settings.memory_limit_mb} MB · upload fino a ${detail.site.settings.upload_max_mb} MB · ${detail.site.settings.max_execution_time_s} s per richiesta · ${detail.site.settings.max_children} processi`}>
			{#if detail.can_manage}<Button variant="outline" size="sm" href="?tab=settings">Cambia</Button>{/if}
		</SettingRow>
	</SettingsSection>
	{#if session.user?.is_admin}
		<SettingsSection title="Zona pericolosa">
			<SettingRow label="Elimina il sito" help="Prima un backup finale, poi route, pool, file (nel cestino per 7 giorni), database e utente.">
				<AlertDialog.Root>
					<AlertDialog.Trigger>
						{#snippet child({ props })}
							<Button variant="destructive" size="sm" {...props}>Elimina…</Button>
						{/snippet}
					</AlertDialog.Trigger>
					<AlertDialog.Content>
						<AlertDialog.Header>
							<AlertDialog.Title>Eliminare {detail.domains[0]?.hostname}?</AlertDialog.Title>
							<AlertDialog.Description>Il sito smette di rispondere subito. Il backup finale resta per 30 giorni.</AlertDialog.Description>
						</AlertDialog.Header>
						<AlertDialog.Footer>
							<AlertDialog.Cancel>Annulla</AlertDialog.Cancel>
							<AlertDialog.Action variant="destructive" onclick={remove}>Elimina</AlertDialog.Action>
						</AlertDialog.Footer>
					</AlertDialog.Content>
				</AlertDialog.Root>
			</SettingRow>
		</SettingsSection>
	{/if}
</div>

<Dialog.Root open={deleting !== null} onOpenChange={(o) => !o && (deleting = null)}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Eliminazione in corso</Dialog.Title>
		</Dialog.Header>
		{#if deleting}
			<JobLog id={deleting} ondone={(j) => j.status === 'done' && goto('/')} />
		{/if}
	</Dialog.Content>
</Dialog.Root>

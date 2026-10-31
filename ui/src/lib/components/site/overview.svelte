<script lang="ts">
	import { askConfirm } from '$lib/ask.svelte';
	import { goto } from '$app/navigation';
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import * as Alert from '$lib/components/ui/alert';
	import Credentials from '$lib/components/app/credentials.svelte';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
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
	const wpAdminUrl = $derived(`${(detail.domains.find((d) => d.kind === 'primary') ?? detail.domains[0])?.url?.replace(/\/$/, '')}/wp-admin/`);

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
			{#if !wpAdmin}
				<SettingRow label="Credenziali dell'amministratore" help="Create con il sito. Si vedono una volta sola, entro 24 ore: tienile pronte per il tuo password manager.">
					<Button onclick={revealWpAdmin}>Mostra le credenziali</Button>
				</SettingRow>
			{:else}
				<div class="flex flex-col gap-4 px-4 py-4">
					<Alert.Root>
						<TriangleAlertIcon />
						<Alert.Title>Copiale adesso</Alert.Title>
						<Alert.Description>Lasciata questa pagina la password non si vede più: per cambiarla userai "Password dimenticata" di WordPress.</Alert.Description>
					</Alert.Root>
					<Credentials id="wp-admin" user={wpAdmin.user} password={wpAdmin.password} />
					<div>
						<Button size="sm" href={wpAdminUrl} target="_blank" rel="noopener"><ExternalLinkIcon data-icon="inline-start" /> Apri la bacheca di WordPress</Button>
					</div>
				</div>
			{/if}
		</SettingsSection>
	{/if}
	<!-- Uno spazio non ha pagine né domini pubblici: niente controlli, domini o cloni. -->
	{#if !detail.site.space}
		<MonitorCard id={detail.site.id} />
		<DomainsCard {detail} {onchange} />
	{/if}
	{#if detail.can_manage}
		<StateCard {detail} {onchange} />
		{#if !detail.site.space}<CopiesCard {detail} />{/if}
	{/if}
	<SettingsSection title="PHP" description={`Pool dedicato con l'utente Unix ${detail.site.slug}.`}>
		<SettingRow label={`PHP ${detail.site.php_version}`} help={`memory_limit ${detail.site.settings.memory_limit_mb} MB · upload fino a ${detail.site.settings.upload_max_mb} MB · ${detail.site.settings.max_execution_time_s} s per richiesta · ${detail.site.settings.max_children} processi`}>
			{#if detail.can_manage}<Button variant="outline" size="sm" href="?tab=settings">Cambia</Button>{/if}
		</SettingRow>
	</SettingsSection>
	{#if session.user?.is_admin}
		<SettingsSection title="Zona pericolosa">
			<SettingRow label="Elimina il sito" help="Prima un backup finale, poi route, pool, file (nel cestino per 7 giorni), database e utente.">
				<Button variant="destructive" size="sm" onclick={async () => (await askConfirm({ title: `Eliminare ${detail.domains[0]?.hostname}?`, description: `Il sito smette di rispondere subito. Il backup finale resta per 30 giorni.`, confirmLabel: 'Elimina', destructive: true })) && remove()}>Elimina…</Button>
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

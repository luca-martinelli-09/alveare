<script lang="ts">
	import { page } from '$app/state';
	import * as Tabs from '$lib/components/ui/tabs';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import StatusBadge from '$lib/components/app/status-badge.svelte';
	import Overview from '$lib/components/site/overview.svelte';
	import Files from '$lib/components/site/files.svelte';
	import Logs from '$lib/components/site/logs.svelte';
	import Database from '$lib/components/site/database.svelte';
	import Backups from '$lib/components/site/backups.svelte';
	import Activity from '$lib/components/site/activity.svelte';
	import Settings from '$lib/components/site/settings.svelte';
	import Updates from '$lib/components/site/updates.svelte';
	import Crons from '$lib/components/site/crons.svelte';
	import { api } from '$lib/api';
	import type { SiteDetail } from '$lib/components/site/types';

	const id = $derived(Number(page.params.id));
	let detail = $state<SiteDetail | null>(null);

	async function load() {
		detail = await api.get<SiteDetail>(`/api/sites/${id}`);
	}

	$effect(() => {
		load();
	});
</script>

{#if !detail}
	<Skeleton class="h-10 w-72" />
	<Skeleton class="h-64 w-full" />
{:else}
	<div class="flex items-center gap-3">
		<h1 class="text-2xl font-semibold">{detail.domains[0]?.hostname ?? detail.site.slug}</h1>
		<StatusBadge status={detail.site.status} />
	</div>
	<Tabs.Root value="overview">
		<Tabs.List>
			<Tabs.Trigger value="overview">Panoramica</Tabs.Trigger>
			{#if detail.can_manage}
				<Tabs.Trigger value="files">File</Tabs.Trigger>
			{/if}
			<Tabs.Trigger value="logs">Log</Tabs.Trigger>
			{#if detail.can_manage}
				<Tabs.Trigger value="database">Database</Tabs.Trigger>
			{/if}
			<Tabs.Trigger value="backups">Backup</Tabs.Trigger>
			{#if detail.site.kind === 'wordpress'}
				<Tabs.Trigger value="updates">Aggiornamenti</Tabs.Trigger>
			{/if}
			<Tabs.Trigger value="cron">Cron</Tabs.Trigger>
			{#if detail.can_manage}
				<Tabs.Trigger value="settings">Impostazioni</Tabs.Trigger>
			{/if}
			<Tabs.Trigger value="activity">Attività</Tabs.Trigger>
		</Tabs.List>
		<Tabs.Content value="overview"><Overview {detail} onchange={load} /></Tabs.Content>
		{#if detail.can_manage}
			<Tabs.Content value="files"><Files {id} /></Tabs.Content>
			<Tabs.Content value="database"><Database {id} /></Tabs.Content>
			<Tabs.Content value="settings"><Settings {detail} onchange={load} /></Tabs.Content>
		{/if}
		<Tabs.Content value="logs"><Logs {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="updates"><Updates {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="cron"><Crons {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="backups"><Backups {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="activity"><Activity {id} /></Tabs.Content>
	</Tabs.Root>
{/if}

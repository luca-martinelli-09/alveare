<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import * as Breadcrumb from '$lib/components/ui/breadcrumb';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import StatusDot from '$lib/components/app/status-dot.svelte';
	import { sites } from '$lib/sites.svelte';
	import { health } from '$lib/health';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import * as Tabs from '$lib/components/ui/tabs';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import Overview from '$lib/components/site/overview.svelte';
	import Files from '$lib/components/site/files.svelte';
	import Logs from '$lib/components/site/logs.svelte';
	import Database from '$lib/components/site/database.svelte';
	import Backups from '$lib/components/site/backups.svelte';
	import Activity from '$lib/components/site/activity.svelte';
	import Settings from '$lib/components/site/settings.svelte';
	import Updates from '$lib/components/site/updates.svelte';
	import Crons from '$lib/components/site/crons.svelte';
	import AppTab from '$lib/components/site/app.svelte';
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

	// La sezione sta nell'URL: ricarica e indietro restano dove eri.
	const tab = $derived(page.url.searchParams.get('tab') ?? 'overview');
	function setTab(v: string) {
		const url = new URL(page.url.href);
		if (v === 'overview') url.searchParams.delete('tab');
		else url.searchParams.set('tab', v);
		goto(url, { replace: true, reset: false });
	}

	const row = $derived(sites.list?.find((s) => s.id === id));
	const h = $derived(row ? health(row) : null);
	const primary = $derived(detail?.domains.find((d) => d.kind === 'primary') ?? detail?.domains[0]);
	const kinds: Record<string, string> = { wordpress: 'WordPress', php: 'PHP', laravel: 'Laravel' };
</script>

{#if !detail}
	<Skeleton class="h-5 w-40" />
	<Skeleton class="h-9 w-72" />
	<Skeleton class="h-64 w-full" />
{:else}
	<Breadcrumb.Root>
		<Breadcrumb.List>
			<Breadcrumb.Item><Breadcrumb.Link href="/">Panoramica</Breadcrumb.Link></Breadcrumb.Item>
			<Breadcrumb.Separator />
			<Breadcrumb.Item><Breadcrumb.Page>{primary?.hostname ?? detail.site.slug}</Breadcrumb.Page></Breadcrumb.Item>
		</Breadcrumb.List>
	</Breadcrumb.Root>
	<header class="-mt-2 flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
		<div class="flex min-w-0 flex-col gap-1.5">
			<h1 class="flex items-center gap-3 text-2xl font-semibold tracking-tight">
				{#if h}<StatusDot level={h.level} label={h.label} class="size-2.5" />{/if}
				<span class="truncate">{primary?.hostname ?? detail.site.slug}</span>
			</h1>
			<p class="text-muted-foreground flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
				<span>{h?.label ?? detail.site.status}</span>
				<span aria-hidden="true">·</span>
				<span>{kinds[detail.site.kind] ?? detail.site.kind}</span>
				<span aria-hidden="true">·</span>
				<span>PHP {detail.site.php_version}</span>
				{#if detail.site.staging}<Badge variant="outline">staging</Badge>{/if}
				{#if h && h.issues.length}<span class="text-warning-text">· {h.issues.join(', ')}</span>{/if}
			</p>
		</div>
		{#if primary}
			<div class="flex flex-wrap gap-2">
				{#if detail.site.kind === 'wordpress'}
					<Button variant="outline" size="sm" href={`${primary.url}/wp-admin/`} target="_blank" rel="noreferrer">Bacheca WordPress</Button>
				{/if}
				<Button size="sm" href={primary.url} target="_blank" rel="noreferrer">Apri il sito <ArrowUpRightIcon data-icon="inline-end" /></Button>
			</div>
		{/if}
	</header>
	<Tabs.Root value={tab} onValueChange={setTab} class="gap-0">
		<div class="-mx-4 overflow-x-auto overflow-y-hidden border-b px-4 [scrollbar-width:none] md:mx-0 md:px-0">
			<Tabs.List variant="line" class="h-10">
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
			{#if detail.can_manage && detail.site.kind !== 'wordpress'}
				<Tabs.Trigger value="app">App</Tabs.Trigger>
			{/if}
			{#if detail.can_manage}
				<Tabs.Trigger value="settings">Impostazioni</Tabs.Trigger>
			{/if}
			<Tabs.Trigger value="activity">Attività</Tabs.Trigger>
			</Tabs.List>
		</div>
		<Tabs.Content value="overview"><Overview {detail} onchange={load} /></Tabs.Content>
		{#if detail.can_manage}
			<Tabs.Content value="files"><Files base={`/api/sites/${id}`} /></Tabs.Content>
			<Tabs.Content value="database"><Database {id} /></Tabs.Content>
			<Tabs.Content value="settings"><Settings {detail} onchange={load} /></Tabs.Content>
		{/if}
		<Tabs.Content value="logs"><Logs {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="updates"><Updates {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="app"><AppTab {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="cron"><Crons {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="backups"><Backups {id} canManage={detail.can_manage} /></Tabs.Content>
		<Tabs.Content value="activity"><Activity {id} /></Tabs.Content>
	</Tabs.Root>
{/if}

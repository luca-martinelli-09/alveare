<script lang="ts">
	import { goto } from '$app/navigation';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import * as Alert from '$lib/components/ui/alert';
	import * as Empty from '$lib/components/ui/empty';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import PageHeader from '$lib/components/app/page-header.svelte';
	import StatusDot from '$lib/components/app/status-dot.svelte';
	import { sites, loadSites } from '$lib/sites.svelte';
	import { health, byHealth, ago, BACKUP_STALE_H } from '$lib/health';
	import SiteActions from '$lib/components/app/site-actions.svelte';
	import JobLog from '$lib/components/app/job-log.svelte';
	import ImportDialog from '$lib/components/app/import-dialog.svelte';
	import { api, errorMessage, formatBytes, type Alert as AlertT, type SiteRow } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import GlobeIcon from '@lucide/svelte/icons/globe';

	let alerts = $state<AlertT[]>([]);
	let templates = $state<{ id: string; name: string }[]>([]);
	let updates = $state<Record<number, number>>({});
	let open = $state(false);
	let domain = $state('');
	let title = $state('');
	let template = $state('wordpress');
	let error = $state('');
	let job = $state<number | null>(null);

	async function load() {
		[, alerts] = await Promise.all([loadSites(), api.get<AlertT[]>('/api/alerts')]);
		const u = await api.get<{ site_id: number; pending: number }[]>('/api/updates');
		updates = Object.fromEntries(u.map((x) => [x.site_id, x.pending]));
	}

	$effect(() => {
		load();
		api.get<typeof templates>('/api/templates').then((t) => (templates = t));
	});

	async function create(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		try {
			const r = await api.post<{ job: number }>('/api/sites', { domain, title, template });
			job = r.job;
		} catch (err) {
			error = errorMessage(err);
		}
	}

	function closeDialog(next: boolean) {
		open = next;
		if (!next) {
			job = null;
			domain = '';
			title = '';
			load();
		}
	}

	const siteName = (id: number | null) => sites.list?.find((s) => s.id === id)?.domain ?? 'Pannello';
	const sorted = $derived((sites.list ?? []).toSorted(byHealth));
	const summary = $derived.by(() => {
		const list = sites.list ?? [];
		const live = list.filter((s) => s.status === 'active');
		const since = Date.now() - BACKUP_STALE_H * 3600_000;
		const backed = live.filter((s) => s.last_backup?.status === 'done' && Date.parse(s.last_backup.started_at) > since).length;
		return [
			{ label: 'Online', value: `${live.filter((s) => s.online).length}/${live.length}`, tone: live.some((s) => s.online === false) ? 'text-destructive' : '' },
			{ label: 'Da guardare', value: String(list.filter((s) => ['down', 'warn'].includes(health(s).level)).length), tone: list.some((s) => ['down', 'warn'].includes(health(s).level)) ? 'text-warning-text' : '' },
			...(live.some((s) => s.backups)
				? [{ label: `Backup nelle ${BACKUP_STALE_H} h`, value: `${backed}/${live.length}`, tone: backed < live.length ? 'text-warning-text' : '' }]
				: [{ label: 'Backup', value: 'spenti', tone: 'text-muted-foreground' }]),
			{ label: 'Aggiornamenti', value: String(Object.values(updates).reduce((a, b) => a + b, 0)), tone: '' }
		];
	});
	const space = (s: SiteRow) => {
		const u = s.monitor?.usage;
		return u ? formatBytes((u.files_bytes ?? 0) + (u.db_bytes ?? 0)) : '–';
	};
	const kinds: Record<string, string> = { wordpress: 'WordPress', php: 'PHP', laravel: 'Laravel' };
</script>

<PageHeader title="Panoramica" description={sites.list ? `${sites.list.length} ${sites.list.length === 1 ? 'sito' : 'siti'} su questo server` : undefined}>
	{#snippet actions()}
	{#if session.user?.is_admin}
			<ImportDialog ondone={load} />
			<Dialog.Root bind:open={() => open, closeDialog}>
				<Dialog.Trigger>
					{#snippet child({ props })}
						<Button {...props}>
							<PlusIcon data-icon="inline-start" />
							Nuovo sito
						</Button>
					{/snippet}
				</Dialog.Trigger>
				<Dialog.Content>
					<Dialog.Header>
						<Dialog.Title>Nuovo sito</Dialog.Title>
						<Dialog.Description>Database, utente, cartella, pool PHP e route vengono creati insieme.</Dialog.Description>
					</Dialog.Header>
					{#if job}
						<JobLog id={job} />
					{:else}
						<form onsubmit={create}>
							<Field.FieldGroup>
								{#if error}
									<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>
								{/if}
								<Field.Field>
									<Field.FieldLabel for="domain">Dominio</Field.FieldLabel>
									<Input id="domain" placeholder="esempio.it" bind:value={domain} required />
								</Field.Field>
								<Field.Field>
									<Field.FieldLabel for="title">Titolo</Field.FieldLabel>
									<Input id="title" bind:value={title} required />
								</Field.Field>
								<Field.Field>
									<Field.FieldLabel>Template</Field.FieldLabel>
									<Select.Root type="single" bind:value={template}>
										<Select.Trigger>{templates.find((t) => t.id === template)?.name ?? template}</Select.Trigger>
										<Select.Content>
											<Select.Group>
												{#each templates as t (t.id)}
													<Select.Item value={t.id} label={t.name}>{t.name}</Select.Item>
												{/each}
											</Select.Group>
										</Select.Content>
									</Select.Root>
								</Field.Field>
								<Dialog.Footer>
									<Button type="submit">Crea sito</Button>
								</Dialog.Footer>
							</Field.FieldGroup>
						</form>
					{/if}
				</Dialog.Content>
			</Dialog.Root>
	{/if}
	{/snippet}
</PageHeader>

{#if sites.list && sites.list.length > 0}
	<dl class="grid grid-cols-2 overflow-hidden rounded-lg border sm:grid-cols-4">
		{#each summary as item, i (item.label)}
			<div class={['flex flex-col gap-1 px-4 py-3', i % 2 === 1 && 'border-l', i >= 2 && 'border-t sm:border-t-0', i === 2 && 'sm:border-l']}>
				<dt class="text-muted-foreground text-xs">{item.label}</dt>
				<dd class={['text-xl font-semibold tabular-nums', item.tone]}>{item.value}</dd>
			</div>
		{/each}
	</dl>
{/if}

{#if alerts.length > 0}
	<section class="flex flex-col gap-2" aria-labelledby="alerts-title">
		<h2 id="alerts-title" class="text-sm font-medium">Avvisi aperti</h2>
		<ul class="divide-y rounded-lg border">
			{#each alerts as alert (alert.id)}
				<li class="flex items-start gap-3 px-4 py-3">
					<TriangleAlertIcon class="text-destructive mt-0.5 size-4 shrink-0" />
					<div class="flex min-w-0 flex-1 flex-col gap-0.5">
						<p class="text-sm">
							{#if alert.site_id}<a class="font-medium hover:underline" href={`/sites/${alert.site_id}`}>{siteName(alert.site_id)}</a>{:else}<span class="font-medium">Pannello</span>{/if}
							<span class="text-muted-foreground"> · {alert.kind}</span>
						</p>
						<p class="text-muted-foreground line-clamp-2 text-sm whitespace-pre-line">{alert.message}</p>
					</div>
					<time class="text-muted-foreground shrink-0 text-xs" datetime={alert.created_at}>{ago(alert.created_at)}</time>
				</li>
			{/each}
		</ul>
	</section>
{/if}

{#if sites.list === null}
	<Skeleton class="h-48 w-full" />
{:else if sites.list.length === 0}
	<Empty.Root class="border">
		<Empty.Header>
			<Empty.Media variant="icon"><GlobeIcon /></Empty.Media>
			<Empty.Title>Nessun sito</Empty.Title>
			<Empty.Description>
				{session.user?.is_admin ? 'Crea il primo sito da un template, o importalo da un altro hosting.' : 'Non hai ancora siti assegnati: chiedi a un amministratore.'}
			</Empty.Description>
		</Empty.Header>
		{#if session.user?.is_admin}
			<Empty.Content><Button onclick={() => (open = true)}><PlusIcon data-icon="inline-start" /> Nuovo sito</Button></Empty.Content>
		{/if}
	</Empty.Root>
{:else}
	<section class="flex flex-col gap-2" aria-labelledby="sites-title">
		<h2 id="sites-title" class="text-sm font-medium">Siti</h2>
		<!-- Telefono: una riga per sito con lo stato in chiaro. -->
		<ul class="divide-y rounded-lg border md:hidden">
			{#each sorted as site (site.id)}
				{@const h = health(site)}
				<li class="flex items-center gap-1 pr-2">
					<a href={`/sites/${site.id}`} class="hover:bg-muted/50 flex min-w-0 flex-1 items-center gap-3 py-3 pl-4">
						<StatusDot level={h.level} label={h.label} />
						<span class="flex min-w-0 flex-1 flex-col">
							<span class="truncate font-medium">{site.domain ?? site.slug}</span>
							<span class="text-muted-foreground truncate text-xs">{h.issues[0] ?? h.label} · backup {site.last_backup ? ago(site.last_backup.started_at) : 'mai'}</span>
						</span>
						{#if (updates[site.id] ?? 0) > 0}<Badge variant="secondary">{updates[site.id]}</Badge>{/if}
					</a>
					<SiteActions {site} />
				</li>
			{/each}
		</ul>
		<div class="hidden rounded-lg border md:block">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head class="pl-4">Sito</Table.Head>
						<Table.Head>Stato</Table.Head>
						<Table.Head>Risposta</Table.Head>
						<Table.Head>Ultimo backup</Table.Head>
						<Table.Head class="text-right">Aggiornamenti</Table.Head>
						<Table.Head class="text-right">Spazio</Table.Head>
						<Table.Head class="text-right">Richieste 1 h</Table.Head>
						<Table.Head class="w-10 pr-2"><span class="sr-only">Azioni</span></Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each sorted as site (site.id)}
						{@const h = health(site)}
						{@const u = site.monitor?.uptime}
						{@const t = site.monitor?.traffic}
						<Table.Row class="group cursor-pointer" onclick={() => goto(`/sites/${site.id}`)}>
							<Table.Cell class="pl-4">
								<a href={`/sites/${site.id}`} class="flex flex-col" onclick={(e) => e.stopPropagation()}>
									<span class="font-medium group-hover:underline">{site.domain ?? site.slug}</span>
									<span class="text-muted-foreground text-xs">{kinds[site.kind] ?? site.kind}{site.staging ? ' · staging' : ''}</span>
								</a>
							</Table.Cell>
							<Table.Cell>
								<span class="flex items-center gap-2" title={h.issues.join('\n')}>
									<StatusDot level={h.level} label={h.label} />
									<span class={['text-sm', h.level === 'down' && 'text-destructive', h.level === 'warn' && 'text-warning-text']}>{h.issues.length && h.level === 'warn' ? h.issues[0] : h.label}</span>
								</span>
							</Table.Cell>
							<Table.Cell class="text-muted-foreground font-mono text-xs">
								{#if u?.status}{u.status} · {u.ms} ms{:else}–{/if}
							</Table.Cell>
							<Table.Cell class={['text-sm', site.last_backup?.status === 'failed' ? 'text-destructive' : 'text-muted-foreground']}>
								{site.last_backup ? (site.last_backup.status === 'failed' ? 'fallito, ' : '') + ago(site.last_backup.started_at) : site.backups ? 'mai' : 'spenti'}
							</Table.Cell>
							<Table.Cell class="text-right">
								{#if (updates[site.id] ?? 0) > 0}<Badge variant="secondary">{updates[site.id]}</Badge>{:else}<span class="text-muted-foreground">–</span>{/if}
							</Table.Cell>
							<Table.Cell class="text-muted-foreground text-right text-sm">{space(site)}</Table.Cell>
							<Table.Cell class="text-muted-foreground text-right text-sm">
								{t?.requests ?? '–'}{#if (t?.errors_5xx ?? 0) > 0}<span class="text-destructive"> · {t?.errors_5xx} 5xx</span>{/if}
							</Table.Cell>
							<Table.Cell class="pr-2" onclick={(e: MouseEvent) => e.stopPropagation()}><SiteActions {site} /></Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
	</section>
{/if}

<script lang="ts">
	import { goto } from '$app/navigation';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import * as Select from '$lib/components/ui/select';
	import * as Alert from '$lib/components/ui/alert';
	import * as Empty from '$lib/components/ui/empty';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import PageHeader from '$lib/components/app/page-header.svelte';
	import StatusPill from '$lib/components/app/status-pill.svelte';
	import SiteAvatar from '$lib/components/app/site-avatar.svelte';
	import UsageBar from '$lib/components/app/usage-bar.svelte';
	import ActivityList, { type Activity } from '$lib/components/app/activity-list.svelte';
	import { page } from '$app/state';
	import { sites, loadSites } from '$lib/sites.svelte';
	import { health, ago, BACKUP_STALE_H } from '$lib/health';
	import { filterSites, sortSites, paginate, type Filters, type SortKey, type SortDir } from '$lib/site-table';
	import * as InputGroup from '$lib/components/ui/input-group';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpDownIcon from '@lucide/svelte/icons/arrow-up-down';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import SiteActions from '$lib/components/app/site-actions.svelte';
	import JobLog from '$lib/components/app/job-log.svelte';
	import ImportDialog from '$lib/components/app/import-dialog.svelte';
	import FirstSteps from '$lib/components/app/first-steps.svelte';
	import { api, errorMessage, type Alert as AlertT, type SiteRow } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import ActivityIcon from '@lucide/svelte/icons/activity';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import ArchiveIcon from '@lucide/svelte/icons/archive';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';

	let alerts = $state<AlertT[]>([]);
	let activity = $state<Activity[] | null>(null);
	let templates = $state<{ id: string; name: string; kind: string }[]>([]);
	let updates = $state<Record<number, number>>({});
	let open = $state(false);
	let domain = $state('');
	let title = $state('');
	// Amministratore di WordPress: proposti "admin" e la tua email, si cambiano sito per sito.
	let adminUser = $state('admin');
	let adminEmail = $state('');
	$effect(() => {
		if (!adminEmail) adminEmail = session.user?.email ?? '';
	});
	const isWordpress = $derived(templates.find((t) => t.id === template)?.kind === 'wordpress');
	let template = $state('wordpress');
	// Spazio: un nome invece del dominio, per script, cron e file senza pagine pubbliche.
	let space = $state(false);
	// Non è un nome di template valido (iniziano con una lettera): niente collisioni.
	const EMPTY = '_vuoto';
	function setSpace(v: boolean) {
		space = v;
		template = v ? EMPTY : 'wordpress';
	}
	let error = $state('');
	let job = $state<number | null>(null);

	async function load() {
		[, alerts, activity] = await Promise.all([loadSites(), api.get<AlertT[]>('/api/alerts'), api.get<Activity[]>('/api/activity')]);
		const u = await api.get<{ site_id: number; pending: number }[]>('/api/updates');
		updates = Object.fromEntries(u.map((x) => [x.site_id, x.pending]));
	}

	// "Nuovo sito" dalla ricerca ⌘K arriva come /?nuovo.
	$effect(() => {
		if (page.url.searchParams.has('nuovo') && session.user?.is_admin) {
			open = true;
			goto('/', { replace: true });
		}
	});

	$effect(() => {
		load();
		api.get<typeof templates>('/api/templates').then((t) => (templates = t));
	});

	async function create(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		try {
			const r = await api.post<{ job: number }>('/api/sites', {
				domain,
				title,
				template: template === EMPTY ? '' : template,
				space,
				...(isWordpress ? { admin_user: adminUser, admin_email: adminEmail } : {})
			});
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
			setSpace(false);
			adminUser = 'admin';
			adminEmail = session.user?.email ?? '';
			load();
		}
	}

	const siteName = (id: number | null) => sites.list?.find((s) => s.id === id)?.domain ?? 'Pannello';
	// Tabella dei siti: filtri, ordinamento per colonna (di partenza: prima i problemi) e pagine.
	const PAGE_SIZE = 20;
	let filters = $state<Filters>({ q: '', status: 'all', kind: 'all' });
	let sort = $state<{ key: SortKey; dir: SortDir }>({ key: 'status', dir: 'asc' });
	let pageNo = $state(1);
	const filtered = $derived(filterSites(sites.list ?? [], filters));
	const shown = $derived(paginate(sortSites(filtered, sort.key, sort.dir), pageNo, PAGE_SIZE));
	const kindsPresent = $derived([...new Set((sites.list ?? []).map((s) => s.kind))]);
	const filtering = $derived(filters.q.trim() !== '' || filters.status !== 'all' || filters.kind !== 'all');
	$effect(() => {
		// Filtri cambiati: si riparte dalla prima pagina.
		[filters.q, filters.status, filters.kind];
		pageNo = 1;
	});
	function sortBy(key: SortKey) {
		// Testo e stato partono in ordine crescente, i numeri dal più grande (più lento, più pieno, più recente).
		sort = sort.key === key ? { key, dir: sort.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: key === 'site' || key === 'status' ? 'asc' : 'desc' };
	}
	const statusLabels: Record<Filters['status'], string> = { all: 'Tutti gli stati', problems: 'Con problemi', ok: 'Online', off: 'Sospesi' };
	const summary = $derived.by(() => {
		const list = sites.list ?? [];
		const live = list.filter((s) => s.status === 'active');
		const since = Date.now() - BACKUP_STALE_H * 3600_000;
		const backed = live.filter((s) => s.last_backup?.status === 'done' && Date.parse(s.last_backup.started_at) > since).length;
		return [
			{ label: 'Online', icon: ActivityIcon, value: `${live.filter((s) => s.online).length}/${live.length}`, tone: live.some((s) => s.online === false) ? 'text-destructive' : '' },
			{ label: 'Da guardare', icon: TriangleAlertIcon, value: String(list.filter((s) => ['down', 'warn'].includes(health(s).level)).length), tone: list.some((s) => ['down', 'warn'].includes(health(s).level)) ? 'text-warning-text' : '' },
			...(live.some((s) => s.backups)
				? [{ label: `Backup nelle ${BACKUP_STALE_H} h`, icon: ArchiveIcon, value: `${backed}/${live.length}`, tone: backed < live.length ? 'text-warning-text' : '' }]
				: [{ label: 'Backup', icon: ArchiveIcon, value: 'spenti', tone: 'text-muted-foreground' }]),
			{ label: 'Aggiornamenti', icon: RefreshCwIcon, value: String(Object.values(updates).reduce((a, b) => a + b, 0)), tone: '' }
		];
	});
	const used = (s: SiteRow) => {
		const u = s.monitor?.usage;
		return u ? (u.files_bytes ?? 0) + (u.db_bytes ?? 0) : null;
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
						<Button variant="primary" {...props}>
							<PlusIcon data-icon="inline-start" />
							Nuovo sito
						</Button>
					{/snippet}
				</Dialog.Trigger>
				<Dialog.Content>
					<Dialog.Header>
						<Dialog.Title>{space ? 'Nuovo spazio' : 'Nuovo sito'}</Dialog.Title>
						<Dialog.Description>
							{space
								? 'Senza dominio né pagine pubbliche: file, script PHP, cron e comandi, con il suo database. I cron HTTP possono chiamare qualunque URL pubblico.'
								: 'Database, utente, cartella, pool PHP e route vengono creati insieme.'}
						</Dialog.Description>
					</Dialog.Header>
					{#if job}
						<JobLog id={job} />
					{:else}
						<form onsubmit={create}>
							<Field.FieldGroup>
								{#if error}
									<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>
								{/if}
								<ToggleGroup.Root type="single" variant="outline" size="sm" value={space ? 'space' : 'site'} onValueChange={(v) => v && setSpace(v === 'space')}>
									<ToggleGroup.Item value="site">Sito</ToggleGroup.Item>
									<ToggleGroup.Item value="space">Spazio</ToggleGroup.Item>
								</ToggleGroup.Root>
								{#if space}
									<Field.Field>
										<Field.FieldLabel for="domain">Nome</Field.FieldLabel>
										<Input id="domain" placeholder="report-soci" bind:value={domain} required pattern="[a-z0-9]([a-z0-9\-]*[a-z0-9])?" maxlength={40} autocomplete="off" spellcheck={false} />
										<Field.FieldDescription>Lettere minuscole, numeri e trattini. Nel pannello diventa {domain || 'nome'}.internal.</Field.FieldDescription>
									</Field.Field>
								{:else}
									<Field.Field>
										<Field.FieldLabel for="domain">Dominio</Field.FieldLabel>
										<Input id="domain" placeholder="esempio.it" bind:value={domain} required />
									</Field.Field>
									<Field.Field>
										<Field.FieldLabel for="title">Titolo</Field.FieldLabel>
										<Input id="title" bind:value={title} required />
									</Field.Field>
								{/if}
								<Field.Field>
									<Field.FieldLabel>Template</Field.FieldLabel>
									<Select.Root type="single" bind:value={template}>
										<Select.Trigger>{template === EMPTY ? 'Vuoto' : (templates.find((t) => t.id === template)?.name ?? template)}</Select.Trigger>
										<Select.Content>
											<Select.Group>
												{#if space}<Select.Item value={EMPTY} label="Vuoto">Vuoto: solo .env e database</Select.Item>{/if}
												{#each templates as t (t.id)}
													<Select.Item value={t.id} label={t.name}>{t.name}</Select.Item>
												{/each}
											</Select.Group>
										</Select.Content>
									</Select.Root>
								</Field.Field>
								{#if isWordpress}
									<div class="grid gap-4 sm:grid-cols-2">
										<Field.Field>
											<Field.FieldLabel for="admin-user">Utente amministratore</Field.FieldLabel>
											<Input id="admin-user" bind:value={adminUser} autocomplete="off" spellcheck={false} required minlength={3} maxlength={60} pattern="[A-Za-z0-9._@\-]+" />
										</Field.Field>
										<Field.Field>
											<Field.FieldLabel for="admin-email">Email amministratore</Field.FieldLabel>
											<Input id="admin-email" type="email" bind:value={adminEmail} required />
										</Field.Field>
									</div>
									<p class="text-muted-foreground -mt-2 text-xs">La password si genera da sola e si vede una volta nella panoramica del sito.</p>
								{/if}
								<Dialog.Footer>
									<Button variant="primary" type="submit">{space ? 'Crea spazio' : 'Crea sito'}</Button>
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
	<dl class="bg-card grid grid-cols-2 overflow-hidden rounded-lg border sm:grid-cols-4">
		{#each summary as item, i (item.label)}
			<div class={['flex flex-col gap-1.5 px-4 py-3', i % 2 === 1 && 'border-l', i >= 2 && 'border-t sm:border-t-0', i === 2 && 'sm:border-l']}>
				<dt class="text-muted-foreground flex items-center gap-2 text-xs">
					<span class="bg-honey-soft text-honey-ink flex size-6 items-center justify-center rounded-md" aria-hidden="true"><item.icon class="size-3.5" /></span>
					{item.label}
				</dt>
				<dd class={['text-xl font-semibold tabular-nums', item.tone]}>{item.value}</dd>
			</div>
		{/each}
	</dl>

	<div class="grid gap-4 lg:grid-cols-2">
		<section class={['flex min-w-0 flex-col gap-2', alerts.length === 0 && 'max-lg:hidden']} aria-labelledby="alerts-title">
			<h2 id="alerts-title" class="text-sm font-medium">Avvisi aperti</h2>
			<ul class="bg-card divide-y rounded-lg border">
				{#each alerts as alert (alert.id)}
					<li class="flex items-start gap-3 px-4 py-3">
						<span class="bg-destructive mt-1.5 size-2 shrink-0 rounded-full" aria-hidden="true"></span>
						<div class="flex min-w-0 flex-1 flex-col gap-0.5">
							<p class="text-sm">
								{#if alert.site_id}<a class="font-medium hover:underline" href={`/sites/${alert.site_id}`}>{siteName(alert.site_id)}</a>{:else}<span class="font-medium">Pannello</span>{/if}
								<span class="text-muted-foreground"> · {alert.kind}</span>
							</p>
							<p class="text-muted-foreground line-clamp-2 text-sm whitespace-pre-line">{alert.message}</p>
						</div>
						<time class="text-muted-foreground shrink-0 text-xs" datetime={alert.created_at}>{ago(alert.created_at)}</time>
					</li>
				{:else}
					<li class="text-muted-foreground px-4 py-6 text-center text-sm">Nessun avviso: tutto tranquillo.</li>
				{/each}
			</ul>
		</section>
		<section class="flex min-w-0 flex-col gap-2" aria-labelledby="activity-title">
			<h2 id="activity-title" class="text-sm font-medium">Attività recente</h2>
			{#if activity === null}<Skeleton class="h-32 w-full" />{:else}<ActivityList items={activity.slice(0, 6)} {siteName} />{/if}
		</section>
	</div>
{/if}

{#if sites.list === null}
	<Skeleton class="h-48 w-full" />
{:else if sites.list.length === 0 && session.user?.is_admin}
	<FirstSteps oncreate={() => (open = true)} />
{:else if sites.list.length === 0}
	<Empty.Root class="bg-card flex-none border">
		<Empty.Header>
			<Empty.Media variant="icon"><GlobeIcon /></Empty.Media>
			<Empty.Title>Nessun sito</Empty.Title>
			<Empty.Description>
				Non hai ancora siti assegnati: chiedi a un amministratore.
			</Empty.Description>
		</Empty.Header>
	</Empty.Root>
{:else}
	<section class="flex flex-col gap-2" aria-labelledby="sites-title">
		<div class="flex flex-wrap items-center gap-2">
			<h2 id="sites-title" class="mr-auto text-sm font-medium">Siti</h2>
			<InputGroup.Root class="w-full sm:w-56">
				<InputGroup.Addon><SearchIcon /></InputGroup.Addon>
				<InputGroup.Input placeholder="Cerca un dominio" aria-label="Cerca un dominio" bind:value={filters.q} />
			</InputGroup.Root>
			<Select.Root type="single" bind:value={filters.status}>
				<Select.Trigger size="sm" class="w-40" aria-label="Filtra per stato">{statusLabels[filters.status]}</Select.Trigger>
				<Select.Content>
					<Select.Group>
						{#each Object.entries(statusLabels) as [value, label] (value)}<Select.Item {value} {label}>{label}</Select.Item>{/each}
					</Select.Group>
				</Select.Content>
			</Select.Root>
			{#if kindsPresent.length > 1}
				<Select.Root type="single" bind:value={filters.kind}>
					<Select.Trigger size="sm" class="w-36" aria-label="Filtra per tipo">{filters.kind === 'all' ? 'Tutti i tipi' : (kinds[filters.kind] ?? filters.kind)}</Select.Trigger>
					<Select.Content>
						<Select.Group>
							<Select.Item value="all" label="Tutti i tipi">Tutti i tipi</Select.Item>
							{#each kindsPresent as k (k)}<Select.Item value={k} label={kinds[k] ?? k}>{kinds[k] ?? k}</Select.Item>{/each}
						</Select.Group>
					</Select.Content>
				</Select.Root>
			{/if}
		</div>
		<!-- Telefono: una riga per sito, stato sul pallino dell'iniziale. -->
		<ul class="bg-card divide-y rounded-lg border md:hidden">
			{#each shown.rows as site (site.id)}
				{@const h = health(site)}
				<li class="flex items-center gap-1 pr-2">
					<a href={`/sites/${site.id}`} class="hover:bg-muted/50 flex min-w-0 flex-1 items-center gap-3 py-2.5 pl-3">
						<SiteAvatar name={site.domain ?? site.slug} url={site.staging ? null : site.url} health={h} />
						<span class="flex min-w-0 flex-1 flex-col">
							<span class="truncate font-medium">{site.domain ?? site.slug}</span>
							<span class={['truncate text-xs', h.level === 'down' ? 'text-destructive' : h.level === 'warn' ? 'text-warning-text' : 'text-muted-foreground']}>
								{h.issues[0] ?? h.label}{site.monitor?.uptime?.ms && h.level === 'ok' ? ` · ${site.monitor.uptime.ms} ms` : ''}
							</span>
						</span>
						{#if (updates[site.id] ?? 0) > 0}<Badge variant="secondary">{updates[site.id]}</Badge>{/if}
					</a>
					<SiteActions {site} />
				</li>
			{/each}
		</ul>
		<div class="bg-card hidden rounded-lg border md:block">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						{@render head('site', 'Sito', 'pl-4')}
						{@render head('status', 'Stato')}
						{@render head('response', 'Risposta')}
						{@render head('backup', 'Ultimo backup')}
						{@render head('space', 'Spazio', 'w-44')}
						{@render head('requests', 'Richieste 1 h', 'text-right', true)}
						<Table.Head class="w-20 pr-2"><span class="sr-only">Azioni</span></Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each shown.rows as site (site.id)}
						{@const h = health(site)}
						{@const u = site.monitor?.uptime}
						{@const t = site.monitor?.traffic}
						<Table.Row class="group cursor-pointer" onclick={() => goto(`/sites/${site.id}`)}>
							<Table.Cell class="pl-4">
								<a href={`/sites/${site.id}`} class="flex items-center gap-2.5" onclick={(e) => e.stopPropagation()}>
									<SiteAvatar name={site.domain ?? site.slug} url={site.staging ? null : site.url} />
									<span class="flex flex-col leading-tight">
										<span class="font-medium group-hover:underline">{site.domain ?? site.slug}</span>
										<span class="text-muted-foreground text-xs">
											{site.space ? 'Spazio' : (kinds[site.kind] ?? site.kind)}{site.staging ? ' · staging' : ''}{(updates[site.id] ?? 0) > 0 ? ` · ${updates[site.id]} aggiornamenti` : ''}
										</span>
									</span>
								</a>
							</Table.Cell>
							<Table.Cell>
								<span title={h.issues.join('\n')}><StatusPill level={h.level} label={h.level === 'warn' ? h.issues[0] : h.label} /></span>
							</Table.Cell>
							<Table.Cell class="text-muted-foreground font-mono text-xs">
								{#if u?.status}{u.status} · {u.ms} ms{:else}–{/if}
							</Table.Cell>
							<Table.Cell class={['text-sm', site.last_backup?.status === 'failed' ? 'text-destructive' : 'text-muted-foreground']}>
								{site.last_backup ? (site.last_backup.status === 'failed' ? 'fallito, ' : '') + ago(site.last_backup.started_at) : site.backups ? 'mai' : 'spenti'}
							</Table.Cell>
							<Table.Cell><UsageBar bytes={used(site)} limitMb={site.disk_alert_mb} /></Table.Cell>
							<Table.Cell class="text-muted-foreground text-right text-sm">
								{t?.requests ?? '–'}{#if (t?.errors_5xx ?? 0) > 0}<span class="text-destructive"> · {t?.errors_5xx} 5xx</span>{/if}
							</Table.Cell>
							<Table.Cell class="pr-2" onclick={(e: MouseEvent) => e.stopPropagation()}>
								<div class="flex justify-end">
									{#if site.url}
										<Button variant="ghost" size="icon-sm" href={site.url} target="_blank" rel="noreferrer" aria-label={`Apri ${site.domain ?? site.slug}`}><ArrowUpRightIcon /></Button>
									{/if}
									<SiteActions {site} />
								</div>
							</Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row><Table.Cell colspan={7} class="text-muted-foreground py-8 text-center">Nessun sito con questi filtri.</Table.Cell></Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
		{#if filtering || shown.pages > 1}
			<div class="text-muted-foreground flex flex-wrap items-center justify-between gap-2 text-xs">
				<span>
					{shown.total ? `${shown.from}–${shown.to} di ${shown.total}` : 'Nessun sito'}{filtering ? ` (filtrati su ${sites.list?.length ?? 0})` : ''}
					{#if filtering}<Button variant="link" size="xs" onclick={() => (filters = { q: '', status: 'all', kind: 'all' })}>Togli i filtri</Button>{/if}
				</span>
				{#if shown.pages > 1}
					<div class="flex items-center gap-1">
						<Button variant="ghost" size="icon-sm" aria-label="Pagina precedente" disabled={shown.page === 1} onclick={() => (pageNo = shown.page - 1)}><ChevronLeftIcon /></Button>
						<span class="tabular-nums">Pagina {shown.page} di {shown.pages}</span>
						<Button variant="ghost" size="icon-sm" aria-label="Pagina successiva" disabled={shown.page === shown.pages} onclick={() => (pageNo = shown.page + 1)}><ChevronRightIcon /></Button>
					</div>
				{/if}
			</div>
		{/if}
	</section>

	{#snippet head(key: SortKey, label: string, cls = '', right = false)}
		<Table.Head class={cls} aria-sort={sort.key === key ? (sort.dir === 'asc' ? 'ascending' : 'descending') : 'none'}>
			<button type="button" class={['hover:text-foreground -mx-1 inline-flex items-center gap-1 rounded px-1', right && 'flex-row-reverse']} onclick={() => sortBy(key)}>
				{label}
				{#if sort.key !== key}<ArrowUpDownIcon class="size-3.5 opacity-40" />{:else if sort.dir === 'asc'}<ArrowUpIcon class="size-3.5" />{:else}<ArrowDownIcon class="size-3.5" />{/if}
			</button>
		</Table.Head>
	{/snippet}
{/if}

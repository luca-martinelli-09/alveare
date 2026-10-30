<script lang="ts">
	import * as Table from '$lib/components/ui/table';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import * as Select from '$lib/components/ui/select';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { api } from '$lib/api';
	import BlockDialog from '$lib/components/app/block-dialog.svelte';
	import BanIcon from '@lucide/svelte/icons/ban';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import PlayIcon from '@lucide/svelte/icons/play';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import SearchIcon from '@lucide/svelte/icons/search';

	let { id, canManage = false }: { id: number; canManage?: boolean } = $props();
	let blocking = $state<string | null>(null);
	type Access = { ts: number; ip: string; method: string; uri: string; status: number; duration_ms: number; user_agent: string };
	type Group = { message: string; count: number; last: string };
	type Kind = 'access' | 'php-error' | 'php-slow' | 'wp-debug';

	let kind = $state<Kind>('access');
	let entries = $state<Access[]>([]);
	let lines = $state<string[]>([]);
	let groups = $state<Group[] | null>(null);
	let live = $state(true);
	let grouped = $state(false);
	let query = $state('');
	let status = $state('');
	// Ricerca applicata (anche nei log ruotati): finché c'è, niente tail in tempo reale.
	let applied = $state({ q: '', status: '' });
	const searching = $derived(applied.q !== '' || applied.status !== '');
	const MAX = 500;
	const statuses = { '': 'Tutti gli stati', '2xx': '2xx', '3xx': '3xx', '4xx': '4xx', '5xx': '5xx' };

	$effect(() => {
		const k = kind;
		const { q, status: st } = applied;
		entries = [];
		lines = [];
		const params = new URLSearchParams({ lines: q || st ? '1000' : '200' });
		if (q) params.set('q', q);
		if (st && k === 'access') params.set('status', st);
		api.get<{ entries?: Access[]; lines?: string[] }>(`/api/sites/${id}/logs/${k}?${params}`).then((r) => {
			entries = (r.entries ?? []).reverse();
			lines = (r.lines ?? []).reverse();
		});
		if (!live || k === 'wp-debug' || q || st) return;
		// Tail in tempo reale via Server-Sent Events.
		const source = new EventSource(`/api/sites/${id}/logs/${k}/stream`);
		source.onmessage = (ev) => {
			const data = JSON.parse(ev.data);
			if (data === null) return;
			if (k === 'access') entries = [data, ...entries].slice(0, MAX);
			else lines = [data, ...lines].slice(0, MAX);
		};
		return () => source.close();
	});

	$effect(() => {
		if (kind === 'php-error' && grouped) {
			groups = null;
			api.get<Group[]>(`/api/sites/${id}/logs/php-error/groups`).then((g) => (groups = g));
		}
	});

	const statusVariant = (s: number) => (s >= 500 ? 'destructive' : s >= 400 ? 'outline' : 'secondary');

	function search(e: SubmitEvent) {
		e.preventDefault();
		applied = { q: query.trim(), status };
	}

	function reset() {
		query = '';
		status = '';
		applied = { q: '', status: '' };
	}

	async function clearDebug() {
		await api.del(`/api/sites/${id}/logs/wp-debug`);
		lines = [];
	}
</script>

<div class="flex flex-col gap-4 pt-4">
	<div class="flex flex-wrap items-center gap-2">
		<ToggleGroup.Root type="single" bind:value={() => kind, (v) => v && (kind = v as Kind)} variant="outline">
			<ToggleGroup.Item value="access">Accessi</ToggleGroup.Item>
			<ToggleGroup.Item value="php-error">Errori PHP</ToggleGroup.Item>
			<ToggleGroup.Item value="php-slow">Richieste lente</ToggleGroup.Item>
			<ToggleGroup.Item value="wp-debug">debug.log</ToggleGroup.Item>
		</ToggleGroup.Root>
		<div class="ml-auto flex gap-2">
			{#if kind === 'wp-debug'}
				<Button variant="outline" size="sm" onclick={clearDebug}>Svuota</Button>
			{:else}
				{#if kind === 'php-error'}
					<Button variant="outline" size="sm" onclick={() => (grouped = !grouped)}>{grouped ? 'Righe' : 'Raggruppa'}</Button>
				{/if}
				<Button variant="outline" size="sm" disabled={searching} onclick={() => (live = !live)}>
					{#if live && !searching}<PauseIcon data-icon="inline-start" /> Pausa{:else}<PlayIcon data-icon="inline-start" /> Riprendi{/if}
				</Button>
				<Button variant="outline" size="sm" href={`/api/sites/${id}/logs/${kind}/download`} download>
					<DownloadIcon data-icon="inline-start" />
					Scarica
				</Button>
			{/if}
		</div>
	</div>

	{#if kind !== 'wp-debug' && !(kind === 'php-error' && grouped)}
		<form class="flex flex-wrap items-center gap-2" onsubmit={search}>
			<Input class="max-w-xs" placeholder={kind === 'access' ? 'IP, path, user agent…' : 'Testo da cercare…'} bind:value={query} aria-label="Cerca nei log" />
			{#if kind === 'access'}
				<Select.Root type="single" bind:value={status}>
					<Select.Trigger class="w-40" aria-label="Stato">{statuses[status as keyof typeof statuses] ?? status}</Select.Trigger>
					<Select.Content>
						<Select.Group>
							{#each Object.entries(statuses) as [v, label] (v)}
								<Select.Item value={v}>{label}</Select.Item>
							{/each}
						</Select.Group>
					</Select.Content>
				</Select.Root>
			{/if}
			<Button type="submit" variant="secondary" size="sm"><SearchIcon data-icon="inline-start" /> Cerca anche nei file ruotati</Button>
			{#if searching}<Button variant="ghost" size="sm" onclick={reset}>Torna al tempo reale</Button>{/if}
		</form>
	{/if}

	{#if kind === 'php-error' && grouped}
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head class="text-right">Volte</Table.Head>
					<Table.Head>Messaggio</Table.Head>
					<Table.Head>Ultima</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each groups ?? [] as g (g.message)}
					<Table.Row>
						<Table.Cell class="text-right"><Badge variant="secondary">{g.count}</Badge></Table.Cell>
						<Table.Cell class="font-mono text-xs whitespace-pre-wrap">{g.message}</Table.Cell>
						<Table.Cell class="text-muted-foreground text-xs whitespace-nowrap">{g.last}</Table.Cell>
					</Table.Row>
				{:else}
					<Table.Row><Table.Cell colspan={3} class="text-muted-foreground">{groups ? 'Nessun errore.' : 'Caricamento…'}</Table.Cell></Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{:else if kind === 'access'}
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>Ora</Table.Head>
					<Table.Head>IP</Table.Head>
					<Table.Head>Richiesta</Table.Head>
					<Table.Head>Stato</Table.Head>
					<Table.Head>Durata</Table.Head>
					<Table.Head>User agent</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each entries as e, i (i)}
					<Table.Row>
						<Table.Cell class="whitespace-nowrap">{new Date(e.ts * 1000).toLocaleString('it-IT')}</Table.Cell>
						<Table.Cell class="whitespace-nowrap">
							<code>{e.ip}</code>
							{#if canManage}
								<Button variant="ghost" size="icon-sm" aria-label={`Blocca ${e.ip}`} onclick={() => (blocking = e.ip)}><BanIcon /></Button>
							{/if}
						</Table.Cell>
						<Table.Cell class="max-w-md truncate">{e.method} {e.uri}</Table.Cell>
						<Table.Cell><Badge variant={statusVariant(e.status)}>{e.status}</Badge></Table.Cell>
						<Table.Cell>{e.duration_ms} ms</Table.Cell>
						<Table.Cell class="text-muted-foreground max-w-xs truncate">{e.user_agent}</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{:else}
		<pre class="bg-muted max-h-[60vh] overflow-auto rounded p-3 text-xs whitespace-pre-wrap">{lines.join('\n') || 'Nessuna riga.'}</pre>
	{/if}
</div>

<BlockDialog siteId={id} bind:ip={blocking} />

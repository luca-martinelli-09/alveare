<script lang="ts">
	import * as Table from '$lib/components/ui/table';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { api } from '$lib/api';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import PlayIcon from '@lucide/svelte/icons/play';

	let { id }: { id: number } = $props();
	type Access = { ts: number; ip: string; method: string; uri: string; status: number; duration_ms: number; user_agent: string };
	type Kind = 'access' | 'php-error' | 'php-slow' | 'wp-debug';

	let kind = $state<Kind>('access');
	let entries = $state<Access[]>([]);
	let lines = $state<string[]>([]);
	let live = $state(true);
	const MAX = 500;

	$effect(() => {
		const k = kind;
		entries = [];
		lines = [];
		api.get<{ entries?: Access[]; lines?: string[] }>(`/api/sites/${id}/logs/${k}?lines=200`).then((r) => {
			entries = (r.entries ?? []).reverse();
			lines = (r.lines ?? []).reverse();
		});
		if (!live || k === 'wp-debug') return;
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

	const statusVariant = (s: number) => (s >= 500 ? 'destructive' : s >= 400 ? 'outline' : 'secondary');

	async function clearDebug() {
		await api.del(`/api/sites/${id}/logs/wp-debug`);
		lines = [];
	}
</script>

<div class="flex flex-col gap-4 pt-4">
	<div class="flex items-center gap-2">
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
				<Button variant="outline" size="sm" onclick={() => (live = !live)}>
					{#if live}<PauseIcon data-icon="inline-start" /> Pausa{:else}<PlayIcon data-icon="inline-start" /> Riprendi{/if}
				</Button>
			{/if}
		</div>
	</div>

	{#if kind === 'access'}
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
						<Table.Cell class="whitespace-nowrap">{new Date(e.ts * 1000).toLocaleTimeString('it-IT')}</Table.Cell>
						<Table.Cell><code>{e.ip}</code></Table.Cell>
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

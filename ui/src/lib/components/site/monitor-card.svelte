<script lang="ts">
	// Ultimi valori del monitoraggio: raggiungibilità, spazio, traffico dell'ultima ora.
	import * as Card from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { api, formatBytes } from '$lib/api';

	let { id }: { id: number } = $props();
	type Monitor = {
		uptime: { ok?: boolean; status?: number; ms?: number; at?: number; error?: string | null } | null;
		usage: { files_bytes?: number; db_bytes?: number; at?: number } | null;
		traffic: { requests?: number; errors_5xx?: number; slow?: number } | null;
		disk_alert_mb: number;
	};
	let m = $state<Monitor | null>(null);

	$effect(() => {
		api.get<Monitor>(`/api/sites/${id}/monitor`).then((v) => (m = v));
	});
	const ago = (t?: number) => (t ? new Date(t * 1000).toLocaleString('it-IT', { dateStyle: 'short', timeStyle: 'short' }) : '–');
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>Monitoraggio</Card.Title>
		<Card.Description>Controllo ogni 5 minuti attraverso Caddy; spazio misurato ogni notte.</Card.Description>
	</Card.Header>
	<Card.Content>
		<dl class="grid grid-cols-2 gap-2 text-sm">
			<dt class="text-muted-foreground">Raggiungibile</dt>
			<dd>
				{#if m?.uptime?.ok === undefined}–
				{:else if m.uptime.ok}<Badge variant="secondary">sì · {m.uptime.status} in {m.uptime.ms} ms</Badge>
				{:else}<Badge variant="destructive">no · {m.uptime.error ?? m.uptime.status}</Badge>{/if}
				<span class="text-muted-foreground text-xs">{ago(m?.uptime?.at)}</span>
			</dd>
			<dt class="text-muted-foreground">File</dt>
			<dd>{formatBytes(m?.usage?.files_bytes)}</dd>
			<dt class="text-muted-foreground">Database</dt>
			<dd>{formatBytes(m?.usage?.db_bytes)} <span class="text-muted-foreground text-xs">(soglia {m?.disk_alert_mb} MB in tutto)</span></dd>
			<dt class="text-muted-foreground">Ultima ora</dt>
			<dd>{m?.traffic?.requests ?? '–'} richieste, {m?.traffic?.errors_5xx ?? '–'} errori 5xx, {m?.traffic?.slow ?? '–'} lente</dd>
		</dl>
	</Card.Content>
</Card.Root>

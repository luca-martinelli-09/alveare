<script lang="ts">
	// Tempo di risposta nel tempo (dai controlli ogni 5 minuti) e lo stato del sito in breve.
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { api, formatBytes, type Monitor } from '$lib/api';
	import { sites } from '$lib/sites.svelte';
	import { ago } from '$lib/health';

	let { id }: { id: number } = $props();
	type History = { bucket_s: number; points: { at: number; ms: number; ok: number }[]; availability: number | null };
	let hours = $state('24');
	let history = $state<History | null>(null);
	let m = $state<(Monitor & { disk_alert_mb: number }) | null>(null);

	$effect(() => {
		api.get<History>(`/api/sites/${id}/uptime?hours=${hours}`).then((h) => (history = h));
	});
	$effect(() => {
		api.get<Monitor & { disk_alert_mb: number }>(`/api/sites/${id}/monitor`).then((v) => (m = v));
	});

	const row = $derived(sites.list?.find((s) => s.id === id));
	const W = 600, H = 150, TOP = 10;
	const chart = $derived.by(() => {
		const pts = history?.points ?? [];
		if (pts.length < 2) return null;
		const max = Math.max(200, ...pts.map((p) => p.ms)) * 1.1;
		const t0 = pts[0].at, t1 = pts[pts.length - 1].at;
		const x = (t: number) => ((t - t0) / Math.max(1, t1 - t0)) * W;
		const y = (ms: number) => TOP + (H - TOP) * (1 - ms / max);
		const line = pts.map((p) => `${x(p.at).toFixed(1)},${y(p.ms).toFixed(1)}`).join(' ');
		const sorted = pts.map((p) => p.ms).toSorted((a, b) => a - b);
		return {
			line,
			area: `0,${H} ${line} ${W},${H}`,
			max: Math.round(max),
			median: sorted[Math.floor(sorted.length / 2)],
			// Intervalli con controlli falliti: segni rossi sotto il grafico.
			fails: pts.filter((p) => p.ok < 1).map((p) => x(p.at))
		};
	});
	const used = $derived(m?.usage ? (m.usage.files_bytes ?? 0) + (m.usage.db_bytes ?? 0) : null);
	const limit = $derived((m?.disk_alert_mb ?? 10240) * 1024 * 1024);
	const ranges = { '24': '24 h', '168': '7 giorni', '720': '30 giorni' };
</script>

<div class="grid gap-4 lg:grid-cols-3">
	<section class="bg-card flex min-w-0 flex-col gap-3 rounded-lg border p-4 lg:col-span-2" aria-labelledby="response-title">
		<div class="flex flex-wrap items-start justify-between gap-2">
			<div>
				<h2 id="response-title" class="text-sm font-medium">Tempo di risposta</h2>
				<p class="text-muted-foreground text-xs">
					Controllo ogni 5 minuti attraverso Caddy{chart ? ` · mediana ${chart.median} ms` : ''}{history?.availability != null ? ` · disponibile ${history.availability}%` : ''}
				</p>
			</div>
			<ToggleGroup.Root type="single" variant="outline" size="sm" value={hours} onValueChange={(v) => v && (hours = v)}>
				{#each Object.entries(ranges) as [value, label] (value)}
					<ToggleGroup.Item {value}>{label}</ToggleGroup.Item>
				{/each}
			</ToggleGroup.Root>
		</div>
		{#if history === null}
			<Skeleton class="h-40 w-full" />
		{:else if chart}
			<svg viewBox={`0 0 ${W} ${H + 6}`} class="h-40 w-full" preserveAspectRatio="none" role="img" aria-label={`Tempo di risposta: mediana ${chart.median} ms, massimo nel grafico ${chart.max} ms`}>
				<defs>
					<linearGradient id="honey-area" x1="0" y1="0" x2="0" y2="1">
						<stop offset="0" stop-color="var(--honey-2)" stop-opacity="0.35" />
						<stop offset="1" stop-color="var(--honey-2)" stop-opacity="0" />
					</linearGradient>
				</defs>
				<line x1="0" y1={H} x2={W} y2={H} stroke="var(--border)" />
				<polygon points={chart.area} fill="url(#honey-area)" />
				<polyline points={chart.line} fill="none" stroke="var(--honey-ink)" stroke-width="2" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
				{#each chart.fails as fx, i (i)}
					<rect x={fx - 2} y={H + 1} width="4" height="5" fill="var(--destructive)" />
				{/each}
			</svg>
			<div class="text-muted-foreground -mt-1 flex justify-between font-mono text-[11px]">
				<span>{ago(history.points[0].at)}</span><span>adesso</span>
			</div>
		{:else}
			<p class="text-muted-foreground py-10 text-center text-sm">Ancora pochi controlli: il grafico compare dopo i primi minuti.</p>
		{/if}
	</section>

	<section class="bg-card flex flex-col gap-3 rounded-lg border p-4" aria-labelledby="glance-title">
		<h2 id="glance-title" class="text-sm font-medium">In breve</h2>
		<dl class="grid grid-cols-[1fr_auto] gap-x-3 gap-y-2.5 text-sm">
			<dt class="text-muted-foreground">Ultimo controllo</dt>
			<dd class={['text-right', m?.uptime?.ok === false && 'text-destructive']}>
				{#if m?.uptime?.ok === undefined}<span class="text-muted-foreground">nessuno ancora</span>
				{:else if m.uptime.ok}{m.uptime.status} · {m.uptime.ms} ms
				{:else}{m.uptime.error ?? `risposta ${m.uptime.status}`}{/if}
			</dd>
			<dt class="text-muted-foreground">Ultima ora</dt>
			<dd class="text-right tabular-nums">
				{#if m?.traffic}{m.traffic.requests ?? 0} richieste{#if (m.traffic.errors_5xx ?? 0) > 0}<span class="text-destructive"> · {m.traffic.errors_5xx} 5xx</span>{/if}
				{:else}<span class="text-muted-foreground">nessun dato ancora</span>{/if}
			</dd>
			<dt class="text-muted-foreground">Ultimo backup</dt>
			<dd class={['text-right', row?.last_backup?.status === 'failed' && 'text-destructive']}>
				{row?.last_backup ? (row.last_backup.status === 'failed' ? 'fallito, ' : '') + ago(row.last_backup.started_at) : row?.backups ? 'mai' : 'spenti'}
			</dd>
		</dl>
		<div class="flex flex-col gap-1.5 border-t pt-3">
			<div class="flex justify-between text-sm">
				<span class="text-muted-foreground">Spazio</span>
				<span class="tabular-nums">{used === null ? 'misurato stanotte' : `${formatBytes(used)} di ${formatBytes(limit)}`}</span>
			</div>
			<div class="bg-muted flex h-1.5 overflow-hidden rounded-full" aria-hidden="true">
				{#if m?.usage}
					<div class="bg-honey h-full" style:width={`${Math.min(100, ((m.usage.files_bytes ?? 0) / limit) * 100)}%`}></div>
					<div class="bg-honey-1 h-full" style:width={`${Math.min(100, ((m.usage.db_bytes ?? 0) / limit) * 100)}%`}></div>
				{/if}
			</div>
			{#if m?.usage}<p class="text-muted-foreground text-xs">File {formatBytes(m.usage.files_bytes)} · database {formatBytes(m.usage.db_bytes)}</p>{/if}
		</div>
	</section>
</div>

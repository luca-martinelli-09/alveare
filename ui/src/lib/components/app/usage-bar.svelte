<script lang="ts">
	// Spazio del sito rispetto alla soglia d'avviso: miele, arancio scuro oltre il 75%.
	import { formatBytes } from '$lib/api';

	let { bytes, limitMb }: { bytes: number | null; limitMb: number } = $props();
	const ratio = $derived(bytes === null ? 0 : Math.min(1, bytes / (limitMb * 1024 * 1024)));
</script>

<div class="flex items-center gap-2" title={bytes === null ? 'misurato di notte' : `${Math.round(ratio * 100)}% della soglia di ${formatBytes(limitMb * 1024 * 1024)}`}>
	<div class="bg-muted h-1.5 flex-1 overflow-hidden rounded-full">
		<div class={['h-full rounded-full', ratio > 0.75 ? 'bg-warning' : 'bg-honey']} style:width={`${Math.max(ratio * 100, bytes ? 2 : 0)}%`}></div>
	</div>
	<span class="text-muted-foreground min-w-14 text-right text-xs tabular-nums">{bytes === null ? '–' : formatBytes(bytes)}</span>
</div>

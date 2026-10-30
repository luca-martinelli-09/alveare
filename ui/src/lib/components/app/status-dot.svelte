<script lang="ts">
	import { cn } from '$lib/utils';
	import type { Level } from '$lib/health';

	let { level, label, class: className = '' }: { level: Level; label: string; class?: string } = $props();
	const tone: Record<Level, string> = {
		ok: 'bg-success',
		warn: 'bg-warning',
		down: 'bg-destructive',
		busy: 'bg-info motion-safe:animate-pulse',
		off: 'bg-muted-foreground/50',
		unknown: 'bg-transparent ring-1 ring-muted-foreground/60 ring-inset'
	};
</script>

<span role="img" aria-label={label} title={label} class={cn('inline-block size-2 shrink-0 rounded-full', tone[level], className)}></span>

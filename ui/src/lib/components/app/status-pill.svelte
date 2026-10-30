<script lang="ts">
	// Stato del sito in una pillola: icona + parola, così non dipende solo dal colore.
	import { cn } from '$lib/utils';
	import type { Level } from '$lib/health';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import XIcon from '@lucide/svelte/icons/x';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import LoaderIcon from '@lucide/svelte/icons/loader-circle';
	import CircleDashedIcon from '@lucide/svelte/icons/circle-dashed';

	let { level, label, class: className = '' }: { level: Level; label: string; class?: string } = $props();
	const tone: Record<Level, string> = {
		ok: 'bg-success-soft text-success-text',
		warn: 'bg-warning-soft text-warning-text',
		down: 'bg-destructive-soft text-destructive',
		busy: 'bg-muted text-info',
		off: 'bg-muted text-muted-foreground',
		unknown: 'bg-muted text-muted-foreground'
	};
	const icons = { ok: CheckIcon, warn: CircleAlertIcon, down: XIcon, busy: LoaderIcon, off: PauseIcon, unknown: CircleDashedIcon };
	const Icon = $derived(icons[level]);
</script>

<span class={cn('inline-flex items-center gap-1 rounded-full py-0.5 pr-2 pl-1.5 text-xs font-medium whitespace-nowrap', tone[level], className)}>
	<Icon class={cn('size-3 stroke-[2.5]', level === 'busy' && 'motion-safe:animate-spin')} aria-hidden="true" />
	{label}
</span>

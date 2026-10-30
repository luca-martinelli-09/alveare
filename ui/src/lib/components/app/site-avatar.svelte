<script lang="ts">
	// Iniziale del sito su tessera color miele; col pallino di stato su telefono.
	import { cn } from '$lib/utils';
	import StatusDot from './status-dot.svelte';
	import type { Health } from '$lib/health';

	let { name, health, class: className = '' }: { name: string; health?: Health; class?: string } = $props();
</script>

<span
	class={cn('relative flex size-7 shrink-0 items-center justify-center rounded-md border bg-accent text-xs font-semibold text-accent-foreground', className)}
	aria-hidden="true"
>
	{name.charAt(0).toUpperCase()}
	{#if health}<StatusDot level={health.level} label={health.label} class="ring-background absolute -right-1 -bottom-1 size-2.5 ring-2" />{/if}
</span>

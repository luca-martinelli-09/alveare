<script lang="ts">
	// Favicon del sito (o la sua iniziale se non c'è) su tessera; col pallino di stato su telefono.
	import { cn } from '$lib/utils';
	import StatusDot from './status-dot.svelte';
	import type { Health } from '$lib/health';

	let { name, url = null, health, class: className = '' }: { name: string; url?: string | null; health?: Health; class?: string } = $props();
	// L'iniziale resta finché la favicon non è davvero caricata: niente tessera vuota se il sito non ne ha.
	let loaded = $state(false);
	let broken = $state(false);
	const icon = $derived(url && !broken ? `${url.replace(/\/$/, '')}/favicon.ico` : null);
</script>

<span
	class={cn('relative flex size-7 shrink-0 items-center justify-center rounded-md border bg-accent text-xs font-semibold text-accent-foreground', className)}
	aria-hidden="true"
>
	{#if !loaded}{name.charAt(0).toUpperCase()}{/if}
	{#if icon}
		<img src={icon} alt="" class={['size-4 rounded-sm', !loaded && 'hidden']} referrerpolicy="no-referrer" onload={() => (loaded = true)} onerror={() => (broken = true)} />
	{/if}
	{#if health}<StatusDot level={health.level} label={health.label} class="ring-background absolute -right-1 -bottom-1 size-2.5 ring-2" />{/if}
</span>

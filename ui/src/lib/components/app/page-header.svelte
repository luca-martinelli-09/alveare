<script lang="ts">
	import type { Snippet } from 'svelte';

	let { title, description, actions, children }: { title: string; description?: string; actions?: Snippet; children?: Snippet } = $props();
</script>

<header class="flex flex-wrap items-end justify-between gap-x-6 gap-y-3">
	<div class="flex min-w-0 flex-col gap-1">
		<h1 class="truncate text-2xl font-semibold tracking-tight">{title}</h1>
		{#if description}<p class="text-muted-foreground text-sm">{description}</p>{/if}
		{@render children?.()}
	</div>
	{#if actions}<div class="flex flex-wrap items-center gap-2">{@render actions()}</div>{/if}
</header>

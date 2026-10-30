<script lang="ts">
	// Gruppo di impostazioni: titolo sopra, righe separate da filetti in un riquadro.
	import type { Snippet } from 'svelte';

	let { title, description, children }: { title: string; description?: string; children: Snippet } = $props();
</script>

<section class="flex flex-col gap-2">
	<div class="flex flex-col gap-0.5">
		<h2 class="text-sm font-medium">{title}</h2>
		{#if description}<p class="text-muted-foreground text-xs">{description}</p>{/if}
	</div>
	<div class="bg-card divide-y rounded-lg border">{@render children()}</div>
</section>

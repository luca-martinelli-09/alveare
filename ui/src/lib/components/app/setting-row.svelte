<script lang="ts">
	// Riga di impostazione: etichetta e spiegazione a sinistra, controllo a destra (sotto, su telefono).
	import type { Snippet } from 'svelte';

	let { label, help, labelFor, children, extra }: { label: string | Snippet; help?: string; labelFor?: string; children?: Snippet; extra?: Snippet } = $props();
</script>

<div class="flex flex-wrap items-center gap-x-6 gap-y-3 px-4 py-3.5">
	<div class="min-w-0 flex-[1_1_18rem]">
		{#if typeof label === 'string'}
			{#if labelFor}<label for={labelFor} class="font-medium">{label}</label>{:else}<p class="font-medium">{label}</p>{/if}
		{:else}
			{@render label()}
		{/if}
		{#if help}<p class="text-muted-foreground mt-0.5 text-sm">{help}</p>{/if}
		{@render extra?.()}
	</div>
	{#if children}<div class="flex flex-wrap items-center gap-2">{@render children()}</div>{/if}
</div>

<script lang="ts">
	// Cornice delle pagine senza sessione completa: login, invito, reset.
	import type { Snippet } from 'svelte';
	import Logo from './logo.svelte';

	let { children }: { children: Snippet } = $props();
</script>

<div class="bg-muted/40 flex min-h-svh flex-col items-center justify-center gap-6 p-6">
	<a href="/" class="flex items-center gap-2.5">
		<Logo class="size-9" />
		<span class="text-xl font-semibold tracking-tight">Alveare</span>
	</a>
	<div class="w-full max-w-sm">{@render children()}</div>
</div>

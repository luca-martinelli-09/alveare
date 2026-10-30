<script lang="ts">
	// Indirizzo che non corrisponde a nessuna pagina del pannello (link vecchio o scritto a mano).
	import { page } from '$app/state';
	import NotFound from '$lib/components/app/not-found.svelte';
</script>

{#if page.status === 404}
	<NotFound title="Pagina non trovata" description={`Nel pannello non c'è niente all'indirizzo ${page.url.pathname}.`} />
{:else}
	<NotFound title="Qualcosa è andato storto" description={page.error?.message ?? 'Errore inatteso.'} />
{/if}

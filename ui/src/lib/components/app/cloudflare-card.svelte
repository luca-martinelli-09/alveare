<script lang="ts">
	// Cloudflare (admin): account configurati e modello delle regole WAF e rate limit.
	import * as Card from '$lib/components/ui/card';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import CodeEditor from '$lib/components/app/code-editor.svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let accounts = $state<string[]>([]);
	let rules = $state('');
	let saved = $state('');
	let error = $state('');
	let loaded = $state(false);

	$effect(() => {
		api.get<{ accounts: string[]; rules: unknown }>('/api/cloudflare').then((r) => {
			accounts = r.accounts;
			rules = saved = JSON.stringify(r.rules, null, 2);
			loaded = true;
		});
	});

	async function save() {
		error = '';
		try {
			await api.put('/api/cloudflare/rules', { rules: JSON.parse(rules) });
			saved = rules;
			toast.success('Modello salvato: si applica dalla pagina di ogni sito');
		} catch (e) {
			error = errorMessage(e);
		}
	}
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>Cloudflare</Card.Title>
		<Card.Description>
			Con un account configurato, gli hostname dei siti vanno da soli sul tunnel (ingress verso http://caddy:80 e CNAME). Le regole qui sotto si applicano alle zone
			dei siti; quelle messe a mano su Cloudflare restano.
		</Card.Description>
	</Card.Header>
	<Card.Content class="flex flex-col gap-3">
		<div class="flex flex-wrap items-center gap-2 text-sm">
			Account:
			{#each accounts as a (a)}<Badge variant="secondary">{a}</Badge>{:else}<span class="text-muted-foreground">nessuno (CLOUDFLARE_&lt;ETICHETTA&gt;_* nel .env)</span>{/each}
		</div>
		{#if error}<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>{/if}
		{#if loaded}<CodeEditor bind:value={rules} filename="regole.json" />{/if}
	</Card.Content>
	<Card.Footer><Button disabled={rules === saved} onclick={save}>Salva il modello</Button></Card.Footer>
</Card.Root>

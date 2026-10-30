<script lang="ts">
	// Cloudflare (admin): account configurati e modello delle regole, letto come tabella; il JSON si apre solo per modificarlo.
	import * as Alert from '$lib/components/ui/alert';
	import * as Table from '$lib/components/ui/table';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import CodeEditor from '$lib/components/app/code-editor.svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	type Rule = { description?: string; expression?: string; action?: string; ratelimit?: { period?: number; requests_per_period?: number; mitigation_timeout?: number } };
	let accounts = $state<string[]>([]);
	let service = $state('');
	let rules = $state('');
	let saved = $state('');
	let error = $state('');
	let editing = $state(false);

	$effect(() => {
		api.get<{ accounts: string[]; rules: unknown; service: string }>('/api/cloudflare').then((r) => {
			accounts = r.accounts;
			service = r.service;
			rules = saved = JSON.stringify(r.rules, null, 2);
		});
	});

	const phases: Record<string, string> = {
		http_ratelimit: 'Limite di frequenza',
		http_request_firewall_custom: 'Regola personalizzata',
		http_request_firewall_managed: 'Regole gestite',
		http_config_settings: 'Impostazioni',
		http_request_cache_settings: 'Cache'
	};
	const actions: Record<string, string> = { block: 'Blocca', managed_challenge: 'Verifica', js_challenge: 'Verifica JS', challenge: 'Verifica', log: 'Registra', skip: 'Salta' };
	const table = $derived.by(() => {
		try {
			const parsed = JSON.parse(saved || '{}') as Record<string, Rule[]>;
			return Object.entries(parsed).flatMap(([phase, list]) =>
				(Array.isArray(list) ? list : []).map((r) => ({
					name: ((r.description ?? '').replace(/^alveare:\s*/, '') || '(senza nome)').replace(/^./, (c) => c.toUpperCase()),
					kind: phases[phase] ?? phase,
					when: r.expression ?? '',
					action:
						(actions[r.action ?? ''] ?? r.action ?? '') +
						(r.ratelimit ? ` oltre ${r.ratelimit.requests_per_period} in ${r.ratelimit.period} s` : '')
				}))
			);
		} catch {
			return [];
		}
	});

	async function save() {
		error = '';
		try {
			await api.put('/api/cloudflare/rules', { rules: JSON.parse(rules) });
			saved = rules;
			editing = false;
			toast.success('Regole salvate: valgono per i domini che aggiungi da ora');
		} catch (e) {
			error = errorMessage(e);
		}
	}
</script>

<section class="flex flex-col gap-2">
	<div class="flex flex-wrap items-end justify-between gap-3">
		<div class="flex flex-col gap-0.5">
			<h2 class="text-sm font-medium">Cloudflare</h2>
			<p class="text-muted-foreground text-xs">Hostname e regole vanno sulle zone da soli quando aggiungi un dominio; quelle messe a mano su Cloudflare restano.</p>
		</div>
		{#if !editing}<Button size="sm" onclick={() => ((rules = saved), (editing = true))}>Modifica le regole (JSON)</Button>{/if}
	</div>
	<div class="bg-card overflow-hidden rounded-lg border">
		<div class="bg-muted/40 flex flex-wrap items-center gap-2 border-b px-4 py-2.5 text-sm">
			<span class="font-medium">Account</span>
			{#each accounts as a (a)}<Badge variant="secondary">{a}</Badge>{:else}<span class="text-muted-foreground">nessuno: aggiungi CLOUDFLARE_&lt;ETICHETTA&gt;_* nel .env</span>{/each}
			{#if service}<span class="text-muted-foreground ml-auto text-xs">ingress verso {service}</span>{/if}
		</div>
		{#if editing}
			<div class="flex flex-col gap-3 p-4">
				{#if error}<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>{/if}
				<CodeEditor bind:value={rules} filename="regole.json" class="max-h-[28rem] min-h-40" />
				<div class="flex justify-end gap-2">
					<Button variant="ghost" size="sm" onclick={() => ((editing = false), (error = ''))}>Annulla</Button>
					{#if rules !== saved}<Button variant="primary" size="sm" onclick={save}>Salva le regole</Button>{/if}
				</div>
			</div>
		{:else}
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head class="pl-4">Regola</Table.Head>
						<Table.Head>Tipo</Table.Head>
						<Table.Head>Quando</Table.Head>
						<Table.Head class="pr-4">Azione</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each table as r, i (i)}
						<Table.Row>
							<Table.Cell class="pl-4 font-medium">{r.name}</Table.Cell>
							<Table.Cell class="text-muted-foreground">{r.kind}</Table.Cell>
							<Table.Cell class="max-w-md truncate font-mono text-xs" title={r.when}>{r.when}</Table.Cell>
							<Table.Cell class="pr-4"><Badge variant="outline">{r.action}</Badge></Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row><Table.Cell colspan={4} class="text-muted-foreground px-4 py-6 text-center">Nessuna regola nel modello.</Table.Cell></Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}
	</div>
</section>

<script lang="ts">
	// Impostazioni del sito: PHP dal form (gestori), override grezzi (admin), IP bloccati.
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Table from '$lib/components/ui/table';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import OverrideEditor from '$lib/components/app/override-editor.svelte';
	import { api, ApiError, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import type { SiteDetail } from './types';

	let { detail, onchange }: { detail: SiteDetail; onchange: () => void } = $props();
	type Block = { id: number; site_id: number | null; cidr: string; expires_at: number | null; created_at: number };
	const id = $derived(detail.site.id);
	let php = $state({ memory_limit_mb: 0, upload_max_mb: 0, max_execution_time_s: 0, max_children: 0 });
	$effect(() => {
		php = { ...detail.site.settings };
	});
	let warning = $state('');
	let confirmText = $state('');
	let blocks = $state<Block[]>([]);

	async function loadBlocks() {
		blocks = await api.get<Block[]>(`/api/sites/${id}/blocks`);
	}

	$effect(() => {
		loadBlocks();
	});

	async function savePhp(confirm_budget = false) {
		try {
			const r = await api.patch<{ warning: string | null }>(`/api/sites/${id}`, { php, confirm_budget });
			warning = r.warning ?? '';
			confirmText = '';
			toast.success('Impostazioni PHP applicate');
			onchange();
		} catch (e) {
			if (e instanceof ApiError && e.status === 409) confirmText = e.message;
			else toast.error(errorMessage(e));
		}
	}

	async function unblock(b: Block) {
		try {
			await api.del(`/api/blocks/${b.id}`);
			await loadBlocks();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const fields = [
		{ key: 'memory_limit_mb', label: 'memory_limit (MB)', min: 32, max: 4096 },
		{ key: 'upload_max_mb', label: 'Upload massimo (MB)', min: 1, max: 4096 },
		{ key: 'max_execution_time_s', label: 'Tempo massimo (s)', min: 5, max: 3600 },
		{ key: 'max_children', label: 'Processi massimi', min: 1, max: 100 }
	] as const;
	const when = (t: number | null) => (t ? new Date(t * 1000).toLocaleString('it-IT', { dateStyle: 'short', timeStyle: 'short' }) : 'mai');
</script>

<div class="grid gap-4 pt-4">
	<Card.Root>
		<Card.Header>
			<Card.Title>PHP</Card.Title>
			<Card.Description>
				Valori del pool del sito. Il massimo teorico di memoria è processi × memory_limit: oltre la RAM del server compare un avviso.
			</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-4">
			{#if warning}<Alert.Root><Alert.Description>{warning}</Alert.Description></Alert.Root>{/if}
			{#if confirmText}
				<Alert.Root variant="destructive">
					<Alert.Title>Serve una conferma</Alert.Title>
					<Alert.Description>{confirmText}</Alert.Description>
				</Alert.Root>
			{/if}
			<Field.FieldGroup class="grid gap-4 sm:grid-cols-2">
				{#each fields as f (f.key)}
					<Field.Field>
						<Field.FieldLabel for={f.key}>{f.label}</Field.FieldLabel>
						<Input id={f.key} type="number" min={f.min} max={f.max} bind:value={php[f.key]} />
					</Field.Field>
				{/each}
			</Field.FieldGroup>
		</Card.Content>
		<Card.Footer class="flex gap-2">
			<Button onclick={() => savePhp()}>Applica</Button>
			{#if confirmText}<Button variant="destructive" onclick={() => savePhp(true)}>Applica comunque</Button>{/if}
		</Card.Footer>
	</Card.Root>

	{#if session.user?.is_admin}
		<OverrideEditor
			base={`/api/sites/${id}/config/php`}
			title="Direttive PHP extra"
			description="php_value[...] e php_admin_value[...] aggiunte al pool, provate con php-fpm -t. open_basedir, disable_functions e simili sono rifiutati."
			filename="pool.ini"
		/>
		<OverrideEditor
			base={`/api/sites/${id}/config/caddy`}
			title="Caddy del sito"
			description="Snippet dentro il blocco del sito (header, redirect, cache, blocchi di path). Validato con /adapt: niente root o socket di altri siti."
			filename="Caddyfile"
		/>
	{/if}

	<Card.Root>
		<Card.Header>
			<Card.Title>IP bloccati</Card.Title>
			<Card.Description>Si bloccano dalla vista degli accessi. È un blocco sul server: per fermare qualcuno prima del tunnel si usa Cloudflare.</Card.Description>
		</Card.Header>
		<Card.Content>
			<Table.Root>
				<Table.Body>
					{#each blocks as b (b.id)}
						<Table.Row>
							<Table.Cell><code>{b.cidr}</code></Table.Cell>
							<Table.Cell>{b.site_id === null ? 'tutti i siti' : 'questo sito'}</Table.Cell>
							<Table.Cell class="text-muted-foreground">scade: {when(b.expires_at)}</Table.Cell>
							<Table.Cell class="text-right">
								{#if b.site_id !== null || session.user?.is_admin}
									<Button variant="outline" size="sm" onclick={() => unblock(b)}>Sblocca</Button>
								{/if}
							</Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row><Table.Cell class="text-muted-foreground">Nessun IP bloccato.</Table.Cell></Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>
</div>

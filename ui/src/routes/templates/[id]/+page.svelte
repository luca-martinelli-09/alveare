<script lang="ts">
	import { askConfirm } from '$lib/ask.svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import * as Breadcrumb from '$lib/components/ui/breadcrumb';
	import { Button } from '$lib/components/ui/button';
	import Files from '$lib/components/site/files.svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import NotFound from '$lib/components/app/not-found.svelte';

	type Template = { id: string; name: string; kind: string; snapshot: boolean; steps: number };
	const id = $derived(page.params.id ?? '');
	// undefined mentre carica, null se non c'è.
	let template = $state<Template | null | undefined>(undefined);

	$effect(() => {
		api.get<Template[]>('/api/templates').then((all) => (template = all.find((t) => t.id === id) ?? null));
	});

	async function remove() {
		try {
			await api.del(`/api/templates/${id}`);
			toast.success(`Template ${template?.name ?? id} spostato nel cestino dei template`);
			await goto('/templates');
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
</script>

{#if template === null}
	<NotFound title="Template non trovato" description="Forse è stato eliminato: i template eliminati restano in templates/.trash." href="/templates" label="Tutti i template" />
{:else}

<Breadcrumb.Root>
	<Breadcrumb.List>
		<Breadcrumb.Item><Breadcrumb.Link href="/templates">Template</Breadcrumb.Link></Breadcrumb.Item>
		<Breadcrumb.Separator />
		<Breadcrumb.Item><Breadcrumb.Page>{template?.name ?? id}</Breadcrumb.Page></Breadcrumb.Item>
	</Breadcrumb.List>
</Breadcrumb.Root>

<header class="-mt-2 flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
	<div class="flex min-w-0 flex-col gap-1">
		<h1 class="truncate text-2xl font-semibold tracking-tight">{template?.name ?? id}</h1>
		<p class="text-muted-foreground text-sm">
			<span class="font-mono">templates/{id}</span> · {template?.snapshot ? 'copia di un sito: file e database' : 'installazione: file e comandi'} ·
			<code class="text-foreground">template.toml</code> descrive il template, <code class="text-foreground">files/</code> diventa la cartella del sito.
		</p>
	</div>
	<Button variant="outline" size="sm" onclick={async () => (await askConfirm({ title: `Eliminare il template ${template?.name ?? id}?`, description: `I siti già creati non cambiano. La cartella va in templates/.trash, da dove si recupera a mano.`, confirmLabel: 'Elimina', destructive: true })) && remove()}><Trash2Icon data-icon="inline-start" /> Elimina</Button>
</header>

<Files base={`/api/templates/${id}`} />
{/if}

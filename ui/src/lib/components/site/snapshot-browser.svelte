<script lang="ts">
	// Vista di uno snapshot: si naviga tra le cartelle e si ripristina un file o una cartella.
	import * as Table from '$lib/components/ui/table';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FileIcon from '@lucide/svelte/icons/file';
	import { api, errorMessage, formatBytes } from '$lib/api';

	let { id, snapshot, onrestore }: { id: number; snapshot: string; onrestore: (path: string) => void } = $props();
	type Entry = { name: string; is_dir: boolean; size: number; mtime: string };
	let path = $state('public');
	let entries = $state<Entry[] | null>(null);
	let error = $state('');
	const parts = $derived(path.split('/').filter(Boolean));

	$effect(() => {
		const current = path;
		entries = null;
		error = '';
		api
			.get<Entry[]>(`/api/sites/${id}/backups/${snapshot}/files?path=${encodeURIComponent(current)}`)
			.then((e) => (entries = e))
			.catch((e) => (error = errorMessage(e)));
	});
</script>

<div class="flex flex-col gap-2">
	<div class="flex flex-wrap items-center gap-1 text-sm">
		<Button variant="link" size="sm" onclick={() => (path = '')}>sito</Button>
		{#each parts as p, i (i)}
			<span>/</span>
			<Button variant="link" size="sm" onclick={() => (path = parts.slice(0, i + 1).join('/'))}>{p}</Button>
		{/each}
	</div>
	{#if error}
		<p class="text-destructive text-sm">{error}</p>
	{:else if !entries}
		<Skeleton class="h-40 w-full" />
	{:else}
		<div class="max-h-80 overflow-auto">
			<Table.Root>
				<Table.Body>
					{#each entries as e (e.name)}
						{@const full = path ? `${path}/${e.name}` : e.name}
						<Table.Row>
							<Table.Cell>
								{#if e.is_dir}
									<button class="flex items-center gap-2 hover:underline" onclick={() => (path = full)}><FolderIcon />{e.name}</button>
								{:else}
									<span class="flex items-center gap-2"><FileIcon />{e.name}</span>
								{/if}
							</Table.Cell>
							<Table.Cell class="text-right">{e.is_dir ? '' : formatBytes(e.size)}</Table.Cell>
							<Table.Cell class="text-right">
								<Button variant="outline" size="sm" onclick={() => onrestore(full)}>Ripristina</Button>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
	{/if}
</div>

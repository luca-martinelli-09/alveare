<script lang="ts">
	// Cestino del file manager: ciò che è stato eliminato negli ultimi 7 giorni, da rimettere dov'era.
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Empty from '$lib/components/ui/empty';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { askConfirm } from '$lib/ask.svelte';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { ago } from '$lib/health';
	import { toast } from 'svelte-sonner';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FileIcon from '@lucide/svelte/icons/file';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';

	let { base, open = $bindable(false), onrestored }: { base: string; open?: boolean; onrestored: (path: string) => void } = $props();

	type Item = { id: string; name: string; origin: string | null; deleted_at: number; is_dir: boolean; size: number };
	const DAYS = 7;
	let items = $state<Item[] | null>(null);

	async function load() {
		try {
			items = await api.get<Item[]>(`${base}/files/trash`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	$effect(() => {
		if (open) load();
	});

	const left = (t: number) => Math.max(0, Math.ceil((t + DAYS * 86400 - Date.now() / 1000) / 86400));

	async function restore(i: Item) {
		try {
			const r = await api.post<{ path: string }>(`${base}/files/ops`, { op: 'restore', id: i.id });
			toast.success(`${i.name} è tornato in ${r.path}`);
			onrestored(r.path);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function purge(i: Item) {
		if (!(await askConfirm({ title: `Eliminare ${i.name} per sempre?`, description: 'Non si potrà più recuperare dal cestino, solo dai backup.', confirmLabel: 'Elimina per sempre', destructive: true }))) return;
		try {
			await api.post(`${base}/files/ops`, { op: 'purge', id: i.id });
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title>Cestino</Dialog.Title>
			<Dialog.Description>Quello che elimini resta qui {DAYS} giorni, poi si svuota da solo.</Dialog.Description>
		</Dialog.Header>
		{#if items === null}
			<Skeleton class="h-24 w-full" />
		{:else if items.length === 0}
			<Empty.Root class="flex-none border py-10">
				<Empty.Header>
					<Empty.Media variant="icon"><Trash2Icon /></Empty.Media>
					<Empty.Title>Il cestino è vuoto</Empty.Title>
				</Empty.Header>
			</Empty.Root>
		{:else}
			<ul class="-mx-1 flex max-h-[60vh] flex-col divide-y overflow-y-auto">
				{#each items as i (i.id)}
					<li class="flex flex-wrap items-center gap-x-3 gap-y-2 px-1 py-2.5">
						<span class="text-muted-foreground [&_svg]:size-4">{#if i.is_dir}<FolderIcon />{:else}<FileIcon />{/if}</span>
						<div class="min-w-0 flex-[1_1_14rem]">
							<p class="truncate font-medium" title={i.name}>{i.name}</p>
							<p class="text-muted-foreground truncate text-xs" title={i.origin ?? undefined}>
								{#if i.origin}da <span class="font-mono">{i.origin}</span>{:else}percorso non salvato: torna nella radice{/if}
							</p>
							<p class="text-muted-foreground text-xs">
								eliminato {ago(i.deleted_at)}{i.is_dir ? '' : ` · ${formatBytes(i.size)}`} · si svuota tra {left(i.deleted_at)} {left(i.deleted_at) === 1 ? 'giorno' : 'giorni'}
							</p>
						</div>
						<div class="flex gap-1.5">
							<Button size="sm" onclick={() => restore(i)}>Ripristina</Button>
							<Button size="sm" variant="ghost" class="text-destructive" onclick={() => purge(i)}>Elimina</Button>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<script lang="ts">
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import CodeEditor from '$lib/components/app/code-editor.svelte';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { uploadFile } from '$lib/upload';
	import { toast } from 'svelte-sonner';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FileIcon from '@lucide/svelte/icons/file';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';

	let { id }: { id: number } = $props();
	type Entry = { name: string; is_dir: boolean; size: number; modified: number; mode: number };

	let path = $state('');
	let entries = $state<Entry[]>([]);
	let editing = $state<{ path: string; content: string } | null>(null);
	let saving = $state(false);
	let progress = $state<number | null>(null);
	let picker: HTMLInputElement;

	const join = (name: string) => (path ? `${path}/${name}` : name);
	const crumbs = $derived(path ? path.split('/') : []);

	async function load() {
		try {
			entries = await api.get<Entry[]>(`/api/sites/${id}/files?path=${encodeURIComponent(path)}`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	$effect(() => {
		path;
		load();
	});

	async function open(e: Entry) {
		if (e.is_dir) {
			path = join(e.name);
			return;
		}
		try {
			const r = await api.get<{ content: string }>(`/api/sites/${id}/files/content?path=${encodeURIComponent(join(e.name))}`);
			editing = { path: join(e.name), content: r.content };
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}

	async function save() {
		if (!editing) return;
		saving = true;
		try {
			await api.put(`/api/sites/${id}/files/content?path=${encodeURIComponent(editing.path)}`, editing.content);
			toast.success('Salvato');
			editing = null;
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			saving = false;
		}
	}

	async function op(body: Record<string, unknown>) {
		try {
			await api.post(`/api/sites/${id}/files/ops`, body);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	function rename(e: Entry) {
		const to = prompt('Nuovo nome o percorso', join(e.name));
		if (to && to !== join(e.name)) op({ op: 'rename', path: join(e.name), to });
	}

	function mkdir() {
		const name = prompt('Nome della cartella');
		if (name) op({ op: 'mkdir', path: join(name) });
	}

	function chmod(e: Entry) {
		const mode = prompt('Permessi (ottale)', (e.mode & 0o777).toString(8));
		if (mode) op({ op: 'chmod', path: join(e.name), mode: parseInt(mode, 8) });
	}

	async function upload(files: FileList | null) {
		for (const file of files ?? []) {
			try {
				progress = 0;
				await uploadFile(id, join(file.name), file, (f) => (progress = f));
				toast.success(`${file.name} caricato`);
			} catch (e) {
				toast.error(`${file.name}: ${errorMessage(e)}`);
			}
		}
		progress = null;
		picker.value = '';
		await load();
	}
</script>

<div class="flex flex-col gap-4 pt-4">
	<div class="flex flex-wrap items-center gap-2">
		<Button variant="ghost" size="sm" onclick={() => (path = '')}>{'/'}</Button>
		{#each crumbs as crumb, i (i)}
			<span class="text-muted-foreground">/</span>
			<Button variant="ghost" size="sm" onclick={() => (path = crumbs.slice(0, i + 1).join('/'))}>{crumb}</Button>
		{/each}
		<div class="ml-auto flex gap-2">
			<Button variant="outline" size="sm" onclick={mkdir}>
				<FolderPlusIcon data-icon="inline-start" />
				Cartella
			</Button>
			<Button size="sm" onclick={() => picker.click()} disabled={progress !== null}>
				{#if progress !== null}
					<Spinner data-icon="inline-start" />
					{Math.round(progress * 100)}%
				{:else}
					<UploadIcon data-icon="inline-start" />
					Carica
				{/if}
			</Button>
			<input bind:this={picker} type="file" multiple class="hidden" onchange={(e) => upload(e.currentTarget.files)} />
		</div>
	</div>

	<Table.Root>
		<Table.Header>
			<Table.Row>
				<Table.Head>Nome</Table.Head>
				<Table.Head>Dimensione</Table.Head>
				<Table.Head>Modificato</Table.Head>
				<Table.Head>Permessi</Table.Head>
				<Table.Head></Table.Head>
			</Table.Row>
		</Table.Header>
		<Table.Body>
			{#each entries as e (e.name)}
				<Table.Row>
					<Table.Cell>
						<button class="flex items-center gap-2 text-left hover:underline" onclick={() => open(e)}>
							{#if e.is_dir}<FolderIcon />{:else}<FileIcon />{/if}
							{e.name}
						</button>
					</Table.Cell>
					<Table.Cell>{e.is_dir ? '' : formatBytes(e.size)}</Table.Cell>
					<Table.Cell class="whitespace-nowrap">{new Date(e.modified * 1000).toLocaleString('it-IT')}</Table.Cell>
					<Table.Cell><code>{(e.mode & 0o777).toString(8)}</code></Table.Cell>
					<Table.Cell class="text-right">
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<Button variant="ghost" size="icon-sm" {...props} aria-label="Azioni"><EllipsisIcon /></Button>
								{/snippet}
							</DropdownMenu.Trigger>
							<DropdownMenu.Content align="end">
								<DropdownMenu.Group>
									{#if !e.is_dir}
										<DropdownMenu.Item onclick={() => (window.location.href = `/api/sites/${id}/files/download?path=${encodeURIComponent(join(e.name))}`)}>Scarica</DropdownMenu.Item>
									{/if}
									<DropdownMenu.Item onclick={() => rename(e)}>Rinomina o sposta</DropdownMenu.Item>
									<DropdownMenu.Item onclick={() => chmod(e)}>Permessi</DropdownMenu.Item>
									<DropdownMenu.Item variant="destructive" onclick={() => confirm(`Spostare ${e.name} nel cestino?`) && op({ op: 'delete', path: join(e.name) })}>
										Elimina
									</DropdownMenu.Item>
								</DropdownMenu.Group>
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					</Table.Cell>
				</Table.Row>
			{/each}
		</Table.Body>
	</Table.Root>
</div>

<Dialog.Root open={editing !== null} onOpenChange={(o) => !o && (editing = null)}>
	<Dialog.Content class="sm:max-w-5xl">
		<Dialog.Header>
			<Dialog.Title>{editing?.path}</Dialog.Title>
		</Dialog.Header>
		{#if editing}
			<CodeEditor bind:value={editing.content} filename={editing.path} />
		{/if}
		<Dialog.Footer>
			<Button onclick={save} disabled={saving}>
				{#if saving}<Spinner data-icon="inline-start" />{/if}
				Salva
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<script lang="ts">
	import Maximize2Icon from '@lucide/svelte/icons/maximize-2';
	import Minimize2Icon from '@lucide/svelte/icons/minimize-2';
	import XIcon from '@lucide/svelte/icons/x';
	import * as Breadcrumb from '$lib/components/ui/breadcrumb';
	import HouseIcon from '@lucide/svelte/icons/house';
	import { askConfirm, askText } from '$lib/ask.svelte';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import CodeEditor from '$lib/components/app/code-editor.svelte';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { uploadTo } from '$lib/upload';
	import { toast } from 'svelte-sonner';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FileIcon from '@lucide/svelte/icons/file';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';

	// base: /api/sites/<id> o /api/templates/<nome>: stesso file manager.
	let { base }: { base: string } = $props();
	type Entry = { name: string; is_dir: boolean; size: number; modified: number; mode: number };

	let path = $state('');
	let entries = $state<Entry[]>([]);
	let editing = $state<{ path: string; content: string; original: string } | null>(null);
	let fullscreen = $state(false);
	let saving = $state(false);
	let progress = $state<number | null>(null);
	let picker: HTMLInputElement;

	const join = (name: string) => (path ? `${path}/${name}` : name);
	const crumbs = $derived(path ? path.split('/') : []);

	async function load() {
		try {
			entries = await api.get<Entry[]>(`${base}/files?path=${encodeURIComponent(path)}`);
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
			const r = await api.get<{ content: string }>(`${base}/files/content?path=${encodeURIComponent(join(e.name))}`);
			editing = { path: join(e.name), content: r.content, original: r.content };
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}

	async function save() {
		if (!editing) return;
		saving = true;
		try {
			await api.put(`${base}/files/content?path=${encodeURIComponent(editing.path)}`, editing.content);
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
			await api.post(`${base}/files/ops`, body);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function rename(e: Entry) {
		const to = await askText({ title: `Rinomina ${e.name}`, label: 'Nuovo nome o percorso', value: join(e.name), mono: true, confirmLabel: 'Rinomina' });
		if (to && to !== join(e.name)) op({ op: 'rename', path: join(e.name), to });
	}

	async function mkdir() {
		const name = await askText({ title: 'Nuova cartella', description: path ? `Dentro ${path}` : 'Nella radice del sito', label: 'Nome della cartella', confirmLabel: 'Crea' });
		if (name) op({ op: 'mkdir', path: join(name) });
	}

	const isArchive = (name: string) => /\.(zip|tar\.gz|tgz)$/i.test(name);

	async function extract(e: Entry) {
		const to = await askText({ title: `Estrai ${e.name}`, description: 'Vuota = radice del sito. I file con lo stesso nome vengono sovrascritti.', label: 'Cartella di destinazione', value: path, mono: true, allowEmpty: true, confirmLabel: 'Estrai' });
		if (to !== null) op({ op: 'extract', path: join(e.name), to });
	}

	async function compress(e: Entry) {
		const to = await askText({ title: `Comprimi ${e.name}`, label: "Nome dell'archivio (.zip o .tar.gz)", value: join(`${e.name}.zip`), mono: true, confirmLabel: 'Comprimi' });
		if (to) op({ op: 'compress', paths: [join(e.name)], to });
	}

	async function chmod(e: Entry) {
		const mode = await askText({ title: `Permessi di ${e.name}`, description: 'In ottale, per esempio 644 per i file e 755 per le cartelle.', label: 'Permessi', value: (e.mode & 0o777).toString(8), mono: true, confirmLabel: 'Applica' });
		if (mode) op({ op: 'chmod', path: join(e.name), mode: parseInt(mode, 8) });
	}

	async function upload(files: FileList | null) {
		for (const file of files ?? []) {
			try {
				progress = 0;
				await uploadTo(`${base}/files/uploads`, join(file.name), file, (f) => (progress = f));
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
		<Breadcrumb.Root>
			<Breadcrumb.List class="gap-1 sm:gap-1">
				<Breadcrumb.Item>
					{#if crumbs.length}
						<button type="button" class="hover:text-foreground flex items-center rounded p-1" aria-label="Radice del sito" onclick={() => (path = '')}><HouseIcon class="size-4" /></button>
					{:else}
						<span class="text-foreground flex items-center p-1" aria-current="page"><HouseIcon class="size-4" /><span class="sr-only">Radice del sito</span></span>
					{/if}
				</Breadcrumb.Item>
				{#each crumbs as crumb, i (i)}
					<Breadcrumb.Separator />
					<Breadcrumb.Item>
						{#if i === crumbs.length - 1}
							<Breadcrumb.Page class="font-mono text-[13px]">{crumb}</Breadcrumb.Page>
						{:else}
							<button type="button" class="hover:text-foreground font-mono text-[13px]" onclick={() => (path = crumbs.slice(0, i + 1).join('/'))}>{crumb}</button>
						{/if}
					</Breadcrumb.Item>
				{/each}
			</Breadcrumb.List>
		</Breadcrumb.Root>
		<div class="ml-auto flex gap-2">
			<Button size="sm" onclick={mkdir}>
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
				<Table.Head class="hidden sm:table-cell">Dimensione</Table.Head>
				<Table.Head class="hidden md:table-cell">Modificato</Table.Head>
				<Table.Head class="hidden sm:table-cell">Permessi</Table.Head>
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
					<Table.Cell class="hidden sm:table-cell">{e.is_dir ? '' : formatBytes(e.size)}</Table.Cell>
					<Table.Cell class="hidden whitespace-nowrap md:table-cell">{new Date(e.modified * 1000).toLocaleString('it-IT')}</Table.Cell>
					<Table.Cell class="hidden sm:table-cell"><code>{(e.mode & 0o777).toString(8)}</code></Table.Cell>
					<Table.Cell class="text-right">
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<Button variant="ghost" size="icon-sm" {...props} aria-label="Azioni"><EllipsisIcon /></Button>
								{/snippet}
							</DropdownMenu.Trigger>
							<DropdownMenu.Content align="end">
								<DropdownMenu.Group>
									<DropdownMenu.Item onclick={() => (window.location.href = `${base}/files/download?path=${encodeURIComponent(join(e.name))}`)}>
										{e.is_dir ? 'Scarica come zip' : 'Scarica'}
									</DropdownMenu.Item>
									{#if !e.is_dir && isArchive(e.name)}
										<DropdownMenu.Item onclick={() => extract(e)}>Estrai</DropdownMenu.Item>
									{/if}
									<DropdownMenu.Item onclick={() => compress(e)}>Comprimi</DropdownMenu.Item>
									<DropdownMenu.Item onclick={() => rename(e)}>Rinomina o sposta</DropdownMenu.Item>
									<DropdownMenu.Item onclick={() => chmod(e)}>Permessi</DropdownMenu.Item>
									<DropdownMenu.Item variant="destructive" onclick={async () => (await askConfirm({ title: `Spostare ${e.name} nel cestino?`, description: 'Resta recuperabile per 7 giorni.', confirmLabel: 'Sposta nel cestino', destructive: true })) && op({ op: 'delete', path: join(e.name) })}>
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

<Dialog.Root open={editing !== null} onOpenChange={(o) => !o && ((editing = null), (fullscreen = false))}>
	<Dialog.Content
		showCloseButton={false}
		class={fullscreen
			? 'inset-0 flex h-dvh w-full max-w-none translate-x-0 translate-y-0 flex-col gap-4 rounded-none p-4 ring-0 sm:max-w-none'
			: 'sm:max-w-5xl'}
	>
		<Dialog.Header class="flex-row items-center gap-2 space-y-0">
			<Dialog.Title class="min-w-0 flex-1 truncate font-mono text-sm">{editing?.path}</Dialog.Title>
			<Button variant="ghost" size="icon-sm" aria-label={fullscreen ? 'Riduci' : 'Tutto schermo'} title={fullscreen ? 'Riduci' : 'Tutto schermo'} onclick={() => (fullscreen = !fullscreen)}>
				{#if fullscreen}<Minimize2Icon />{:else}<Maximize2Icon />{/if}
			</Button>
			<Button variant="ghost" size="icon-sm" aria-label="Chiudi" title="Chiudi" onclick={() => ((editing = null), (fullscreen = false))}><XIcon /></Button>
		</Dialog.Header>
		{#if editing}
			<CodeEditor bind:value={editing.content} filename={editing.path} class={fullscreen ? 'min-h-0 flex-1' : 'h-[60vh]'} />
		{/if}
		<Dialog.Footer>
			<Button variant="ghost" onclick={() => ((editing = null), (fullscreen = false))}>{editing && editing.content !== editing.original ? 'Annulla' : 'Chiudi'}</Button>
			{#if editing && editing.content !== editing.original}
				<Button variant="primary" onclick={save} disabled={saving}>
					{#if saving}<Spinner data-icon="inline-start" />{/if}
					Salva
				</Button>
			{/if}
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

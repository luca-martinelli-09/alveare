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
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import CodeEditor from '$lib/components/app/code-editor.svelte';
	import TrashDialog from './trash-dialog.svelte';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { enqueue } from '$lib/uploads.svelte';
	import { entriesOf, filesFromEntries } from '$lib/dropped';
	import { toast } from 'svelte-sonner';
	import { cn } from '$lib/utils';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FileIcon from '@lucide/svelte/icons/file';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import FilePlusIcon from '@lucide/svelte/icons/file-plus';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';

	// base: /api/sites/<id> o /api/templates/<nome>: stesso file manager.
	let { base }: { base: string } = $props();
	type Entry = { name: string; is_dir: boolean; size: number; modified: number; mode: number };

	let path = $state('');
	let entries = $state<Entry[]>([]);
	let editing = $state<{ path: string; content: string; original: string } | null>(null);
	let fullscreen = $state(false);
	let trashOpen = $state(false);
	let saving = $state(false);
	let picker: HTMLInputElement;
	let folderPicker: HTMLInputElement;

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
			return true;
		} catch (e) {
			toast.error(errorMessage(e));
			return false;
		}
	}

	// Trascina e rilascia: una riga su una cartella, o su un pezzo del percorso in alto, la sposta lì.
	// Da tastiera e da telefono resta "Rinomina o sposta" nel menu della riga.
	let dragging = $state<string | null>(null);
	let over = $state<string | null>(null);
	const parentOf = (p: string) => p.slice(0, Math.max(p.lastIndexOf('/'), 0));
	const canDrop = (dir: string) => dragging !== null && dir !== dragging && !dir.startsWith(`${dragging}/`) && parentOf(dragging) !== dir;

	function dropTarget(dir: string) {
		return {
			'data-drop': over === dir ? 'true' : undefined,
			ondragover: (ev: DragEvent) => {
				if (fromComputer(ev)) {
					ev.preventDefault();
					ev.stopPropagation();
					over = dir;
					return;
				}
				if (!canDrop(dir)) return;
				ev.preventDefault();
				if (ev.dataTransfer) ev.dataTransfer.dropEffect = 'move';
				over = dir;
			},
			ondragleave: () => {
				if (over === dir) over = null;
			},
			ondrop: async (ev: DragEvent) => {
				if (fromComputer(ev)) return dropFiles(ev, dir);
				ev.preventDefault();
				const from = dragging;
				dragging = over = null;
				if (!from) return;
				const name = from.slice(from.lastIndexOf('/') + 1);
				if (await op({ op: 'rename', path: from, to: dir ? `${dir}/${name}` : name })) toast.success(`${name} spostato in ${dir || 'radice'}`);
			}
		};
	}

	function dragSource(path: string) {
		return {
			draggable: true,
			ondragstart: (ev: DragEvent) => {
				dragging = path;
				ev.dataTransfer?.setData('text/plain', path);
				if (ev.dataTransfer) ev.dataTransfer.effectAllowed = 'move';
			},
			ondragend: () => (dragging = over = null)
		};
	}

	async function rename(e: Entry) {
		const to = await askText({ title: `Rinomina ${e.name}`, label: 'Nuovo nome o percorso', value: join(e.name), mono: true, confirmLabel: 'Rinomina' });
		if (to && to !== join(e.name)) op({ op: 'rename', path: join(e.name), to });
	}

	async function mkdir() {
		const name = await askText({ title: 'Nuova cartella', description: path ? `Dentro ${path}` : 'Nella radice del sito', label: 'Nome della cartella', confirmLabel: 'Crea' });
		if (name) op({ op: 'mkdir', path: join(name) });
	}

	// File vuoto nella cartella corrente, poi subito nell'editor.
	async function newFile() {
		const name = await askText({ title: 'Nuovo file', description: path ? `Dentro ${path}` : 'Nella radice del sito', label: 'Nome del file', placeholder: 'index.php', mono: true, confirmLabel: 'Crea' });
		if (name && (await op({ op: 'newfile', path: join(name) }))) editing = { path: join(name), content: '', original: '' };
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

	// Da una cartella ogni file porta il percorso relativo ("tema/css/a.css"): il server crea le sottocartelle.
	// Le cartelle vuote il browser non le elenca, quindi non arrivano.
	function upload(files: FileList | null) {
		enqueue(`${base}/files/uploads`, [...(files ?? [])].map((file) => ({ file, path: file.webkitRelativePath || file.name })), path, load);
		picker.value = folderPicker.value = '';
	}

	// File e cartelle trascinati dal computer: sulla lista vanno nella cartella aperta, su una riga cartella dentro quella.
	let dropping = $state(false);
	const fromComputer = (ev: DragEvent) => dragging === null && !!ev.dataTransfer?.types.includes('Files');
	async function dropFiles(ev: DragEvent, dir: string) {
		ev.preventDefault();
		ev.stopPropagation();
		dropping = false;
		over = null;
		if (!ev.dataTransfer) return;
		// Entry e file vanno letti subito: dopo il primo await il browser svuota il DataTransfer.
		const entries = entriesOf(ev.dataTransfer);
		const plain = [...ev.dataTransfer.files].map((file) => ({ file, path: file.name }));
		enqueue(`${base}/files/uploads`, entries.length ? await filesFromEntries(entries) : plain, dir, load);
	}
</script>

<div
	class="relative flex flex-col gap-4 pt-4"
	role="region"
	aria-label="File"
	ondragover={(ev) => {
		if (!fromComputer(ev)) return;
		ev.preventDefault();
		dropping = true;
	}}
	ondragleave={(ev) => {
		if (!(ev.currentTarget as HTMLElement).contains(ev.relatedTarget as Node | null)) dropping = false;
	}}
	ondrop={(ev) => fromComputer(ev) && dropFiles(ev, path)}
>
	{#if dropping && over === null}
		<div class="border-primary bg-background/80 pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-lg border-2 border-dashed">
			<p class="flex items-center gap-2 font-medium"><UploadIcon class="size-5" /> Rilascia per caricare in <span class="font-mono">{path || 'radice'}</span></p>
		</div>
	{/if}
	<div class="flex flex-wrap items-center gap-2">
		<Breadcrumb.Root>
			<Breadcrumb.List class="gap-1 sm:gap-1">
				<Breadcrumb.Item>
					{#if crumbs.length}
						<button type="button" class="hover:text-foreground data-[drop=true]:bg-accent data-[drop=true]:text-foreground flex items-center rounded p-1" aria-label="Radice del sito" onclick={() => (path = '')} {...dropTarget('')}><HouseIcon class="size-4" /></button>
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
							<button type="button" class="hover:text-foreground data-[drop=true]:bg-accent data-[drop=true]:text-foreground rounded px-1 font-mono text-[13px]" onclick={() => (path = crumbs.slice(0, i + 1).join('/'))} {...dropTarget(crumbs.slice(0, i + 1).join('/'))}>{crumb}</button>
						{/if}
					</Breadcrumb.Item>
				{/each}
			</Breadcrumb.List>
		</Breadcrumb.Root>
		<div class="ml-auto flex gap-2">
			<Button size="sm" variant="ghost" onclick={() => (trashOpen = true)}><Trash2Icon data-icon="inline-start" /> Cestino</Button>
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<Button size="sm" {...props}><PlusIcon data-icon="inline-start" /> Nuovo</Button>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content align="end">
					<DropdownMenu.Group>
						<DropdownMenu.Item onclick={newFile}><FilePlusIcon /> File</DropdownMenu.Item>
						<DropdownMenu.Item onclick={mkdir}><FolderPlusIcon /> Cartella</DropdownMenu.Item>
					</DropdownMenu.Group>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<Button size="sm" {...props}><UploadIcon data-icon="inline-start" /> Carica</Button>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content align="end">
					<DropdownMenu.Group>
						<DropdownMenu.Item onclick={() => picker.click()}><FileIcon /> Carica file</DropdownMenu.Item>
						<DropdownMenu.Item onclick={() => folderPicker.click()}><FolderIcon /> Carica cartella</DropdownMenu.Item>
					</DropdownMenu.Group>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
			<input bind:this={picker} type="file" multiple class="hidden" onchange={(e) => upload(e.currentTarget.files)} />
			<input bind:this={folderPicker} type="file" webkitdirectory class="hidden" onchange={(e) => upload(e.currentTarget.files)} />
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
				<ContextMenu.Root>
				<ContextMenu.Trigger>
				{#snippet child({ props })}
				<Table.Row
					{...props}
					class={cn('data-[drop=true]:bg-accent data-[state=open]:bg-muted', dragging === join(e.name) && 'opacity-50')}
					{...dragSource(join(e.name))}
					{...e.is_dir ? dropTarget(join(e.name)) : {}}
				>
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
							<DropdownMenu.Content align="end">{@render rowActions(e, DropdownMenu)}
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					</Table.Cell>
				</Table.Row>
				{/snippet}
				</ContextMenu.Trigger>
				<ContextMenu.Content class="w-56">{@render rowActions(e, ContextMenu)}</ContextMenu.Content>
				</ContextMenu.Root>
			{/each}
		</Table.Body>
	</Table.Root>
</div>

<!-- Azioni di una riga: le stesse nel menu "…" e in quello del tasto destro. -->
{#snippet rowActions(e: Entry, M: typeof DropdownMenu | typeof ContextMenu)}
	<M.Group>
		<M.Item onclick={() => open(e)}>{e.is_dir ? 'Apri' : 'Apri nell\'editor'}</M.Item>
		<M.Item onclick={() => (window.location.href = `${base}/files/download?path=${encodeURIComponent(join(e.name))}`)}>
			{e.is_dir ? 'Scarica come zip' : 'Scarica'}
		</M.Item>
		{#if !e.is_dir && isArchive(e.name)}
			<M.Item onclick={() => extract(e)}>Estrai</M.Item>
		{/if}
		<M.Item onclick={() => compress(e)}>Comprimi</M.Item>
		<M.Item onclick={() => rename(e)}>Rinomina o sposta</M.Item>
		<M.Item onclick={() => chmod(e)}>Permessi</M.Item>
	</M.Group>
	<M.Separator />
	<M.Group>
		<M.Item
			variant="destructive"
			onclick={async () =>
				(await askConfirm({ title: `Spostare ${e.name} nel cestino?`, description: "Per 7 giorni lo ritrovi nel Cestino, in alto a destra, e lo rimetti dov'era.", confirmLabel: 'Sposta nel cestino', destructive: true })) &&
				op({ op: 'delete', path: join(e.name) })}
		>
			Elimina
		</M.Item>
	</M.Group>
{/snippet}

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

<TrashDialog {base} bind:open={trashOpen} onrestored={(p) => ((path = parentOf(p)), load())} />

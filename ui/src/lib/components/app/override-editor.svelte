<script lang="ts">
	// Override versionato (PHP o Caddy): editor, salvataggio validato dal pannello, storico con diff e ripristino.
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Table from '$lib/components/ui/table';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import CodeEditor from '$lib/components/app/code-editor.svelte';
	import { api, errorMessage } from '$lib/api';
	import { diffLines } from '$lib/diff';
	import { toast } from 'svelte-sonner';

	let { base, title, description, filename }: { base: string; title: string; description: string; filename: string } = $props();
	type Version = { id: number; version: number; content: string; author_id: number | null; created_at: string };
	let content = $state('');
	let saved = $state('');
	let version = $state(0);
	let history = $state<Version[]>([]);
	let error = $state('');
	let busy = $state(false);
	let comparing = $state<Version | null>(null);
	let loaded = $state(false);
	let editing = $state(false);
	let showHistory = $state(false);
	const lines = $derived(saved.split('\n').filter((l, i, all) => l.trim() || i < all.length - 1));
	const preview = $derived(lines.slice(0, 4).join('\n'));

	async function load() {
		const cur = await api.get<{ content: string; version: number }>(base);
		content = saved = cur.content;
		version = cur.version;
		history = await api.get<Version[]>(`${base}/history`);
		loaded = true;
	}

	$effect(() => {
		base;
		load();
	});

	async function put(body: Record<string, unknown>, done: string) {
		busy = true;
		error = '';
		try {
			await api.put(base, body);
			toast.success(done);
			comparing = null;
			editing = false;
			await load();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<div class="flex flex-col gap-3 px-4 py-3.5">
	<div class="flex flex-wrap items-center gap-x-6 gap-y-3">
		<div class="min-w-0 flex-[1_1_18rem]">
			<p class="font-medium">{title}</p>
			<p class="text-muted-foreground mt-0.5 text-sm">
				{description}
				{#if loaded}
					{saved.trim() ? ` ${lines.length} ${lines.length === 1 ? 'riga' : 'righe'} · v${version}.` : ' Niente di personalizzato.'}
					{#if history.length > 1}
						<button type="button" class="hover:text-foreground underline underline-offset-2" onclick={() => (showHistory = !showHistory)}>
							{showHistory ? 'Nascondi le versioni' : `${history.length} versioni`}
						</button>
					{/if}
				{/if}
			</p>
		</div>
		{#if loaded && !editing}
			<Button size="sm" onclick={() => ((content = saved), (editing = true))}>{saved.trim() ? 'Modifica' : 'Aggiungi'}</Button>
		{/if}
	</div>
	{#if error}
		<Alert.Root variant="destructive"><Alert.Description class="whitespace-pre-wrap">{error}</Alert.Description></Alert.Root>
	{/if}
	{#if editing}
		{#key version}
			<CodeEditor bind:value={content} {filename} class="max-h-96 min-h-28" />
		{/key}
		<div class="flex justify-end gap-2">
			<Button variant="ghost" size="sm" onclick={() => ((editing = false), (error = ''), (content = saved))}>Annulla</Button>
			{#if content !== saved}
				<Button variant="primary" size="sm" disabled={busy} onclick={() => put({ content }, 'Salvato e applicato')}>
					{#if busy}<Spinner data-icon="inline-start" />{/if}
					Valida e applica
				</Button>
			{/if}
		</div>
	{:else if saved.trim()}
		<pre class="bg-muted/60 text-foreground/80 overflow-hidden rounded-md border px-3 py-2 font-mono text-xs leading-relaxed">{preview}{lines.length > 4 ? `\n… altre ${lines.length - 4} righe` : ''}</pre>
	{/if}
	{#if showHistory}
		<div class="rounded-md border">
			<Table.Root>
				<Table.Body>
					{#each history as v (v.id)}
						<Table.Row>
							<Table.Cell>v{v.version}{v.version === version ? ' (attiva)' : ''}</Table.Cell>
							<Table.Cell class="text-muted-foreground">{new Date(v.created_at).toLocaleString('it-IT')} · utente {v.author_id ?? '–'}</Table.Cell>
							<Table.Cell class="text-right">
								{#if v.version !== version}
									<Button variant="ghost" size="sm" onclick={() => (comparing = v)}>Confronta</Button>
									<Button size="sm" disabled={busy} onclick={() => put({ restore: v.version }, `Versione ${v.version} ripristinata`)}>Ripristina</Button>
								{/if}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
	{/if}
</div>

<Dialog.Root open={comparing !== null} onOpenChange={(o) => !o && (comparing = null)}>
	<Dialog.Content class="sm:max-w-3xl">
		<Dialog.Header>
			<Dialog.Title>v{comparing?.version} → attiva (v{version})</Dialog.Title>
		</Dialog.Header>
		{#if comparing}
			<pre class="bg-muted max-h-96 overflow-auto rounded p-3 text-xs">{#each diffLines(comparing.content, saved) as l, i (i)}<div
						class={l.kind === 'add' ? 'text-green-700 dark:text-green-400' : l.kind === 'del' ? 'text-destructive' : 'text-muted-foreground'}>{l.kind === 'add' ? '+ ' : l.kind === 'del' ? '- ' : '  '}{l.text}</div>{/each}</pre>
		{/if}
	</Dialog.Content>
</Dialog.Root>

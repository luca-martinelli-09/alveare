<script lang="ts">
	// Override versionato (PHP o Caddy): editor, salvataggio validato dal pannello, storico con diff e ripristino.
	import * as Card from '$lib/components/ui/card';
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
			await load();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>{title}</Card.Title>
		<Card.Description>{description}</Card.Description>
	</Card.Header>
	<Card.Content class="flex flex-col gap-3">
		{#if error}
			<Alert.Root variant="destructive"><Alert.Description class="whitespace-pre-wrap">{error}</Alert.Description></Alert.Root>
		{/if}
		{#if loaded}
			{#key version}
				<CodeEditor bind:value={content} {filename} />
			{/key}
		{/if}
		{#if history.length}
			<Table.Root>
				<Table.Body>
					{#each history as v (v.id)}
						<Table.Row>
							<Table.Cell>v{v.version}{v.version === version ? ' (attiva)' : ''}</Table.Cell>
							<Table.Cell class="text-muted-foreground">{new Date(v.created_at).toLocaleString('it-IT')} · utente {v.author_id ?? '–'}</Table.Cell>
							<Table.Cell class="text-right">
								{#if v.version !== version}
									<Button variant="ghost" size="sm" onclick={() => (comparing = v)}>Confronta</Button>
									<Button variant="outline" size="sm" disabled={busy} onclick={() => put({ restore: v.version }, `Versione ${v.version} ripristinata`)}>Ripristina</Button>
								{/if}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}
	</Card.Content>
	<Card.Footer>
		<Button disabled={busy || content === saved} onclick={() => put({ content }, 'Override salvato e applicato')}>
			{#if busy}<Spinner data-icon="inline-start" />{/if}
			Valida e applica
		</Button>
	</Card.Footer>
</Card.Root>

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

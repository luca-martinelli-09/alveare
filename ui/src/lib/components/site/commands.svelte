<script lang="ts">
	// Comandi una tantum: WP-CLI, artisan o uno script PHP del sito, con l'utente del sito e senza shell.
	import * as InputGroup from '$lib/components/ui/input-group';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import PlayIcon from '@lucide/svelte/icons/play';

	let { id, siteKind }: { id: number; siteKind: string } = $props();

	type Kind = 'wp' | 'artisan' | 'php';
	const kinds = $derived<{ value: Kind; label: string; prefix: string; examples: string[] }[]>([
		...(siteKind === 'wordpress' ? [{ value: 'wp' as Kind, label: 'WP-CLI', prefix: 'wp', examples: ['plugin list', 'cache flush', 'core verify-checksums', 'user list --role=administrator'] }] : []),
		...(siteKind === 'laravel' ? [{ value: 'artisan' as Kind, label: 'Artisan', prefix: 'php artisan', examples: ['about', 'migrate:status', 'optimize:clear', 'queue:failed'] }] : []),
		{ value: 'php' as Kind, label: 'Script PHP', prefix: 'php', examples: [] }
	]);
	let kind = $state<Kind>('php');
	$effect(() => {
		kind = kinds[0].value;
	});
	const current = $derived(kinds.find((k) => k.value === kind) ?? kinds[0]);

	let command = $state('');
	let job = $state<number | null>(null);
	let running = $state(false);
	let history = $state<{ kind: Kind; command: string }[]>([]);

	async function run(e?: SubmitEvent) {
		e?.preventDefault();
		if (!command.trim()) return;
		running = true;
		try {
			job = (await api.post<{ job: number }>(`/api/sites/${id}/commands`, { kind, command })).job;
			history = [{ kind, command: command.trim() }, ...history.filter((h) => !(h.kind === kind && h.command === command.trim()))].slice(0, 8);
		} catch (err) {
			toast.error(errorMessage(err));
			running = false;
		}
	}
</script>

<div class="flex flex-col gap-4 pt-6">
	<div class="flex flex-col gap-1">
		<h2 class="text-sm font-medium">Esegui un comando</h2>
		<p class="text-muted-foreground text-xs">
			Gira con l'utente del sito, nella sua cartella, senza shell: niente pipe, redirect o comandi di sistema. Invio esegue, Maiusc+Invio va a capo.
		</p>
	</div>
	{#if kinds.length > 1}
		<ToggleGroup.Root type="single" variant="outline" size="sm" value={kind} onValueChange={(v) => v && (kind = v as Kind)}>
			{#each kinds as k (k.value)}<ToggleGroup.Item value={k.value}>{k.label}</ToggleGroup.Item>{/each}
		</ToggleGroup.Root>
	{/if}
	<form onsubmit={run} class="flex flex-col gap-2 sm:flex-row sm:items-start">
		<InputGroup.Root class="flex-1">
			<InputGroup.Addon class="self-start pt-2"><InputGroup.Text class="font-mono">{current.prefix}</InputGroup.Text></InputGroup.Addon>
			<!-- Invio esegue, Maiusc+Invio va a capo (script PHP per eval su più righe). -->
			<InputGroup.Textarea
				class="max-h-80 min-h-9 [field-sizing:content] resize-none font-mono text-sm"
				rows={1}
				aria-label="Comando"
				placeholder={kind === 'php' ? 'public/script.php --opzione' : current.examples[0]}
				bind:value={command}
				autocomplete="off"
				spellcheck={false}
				onkeydown={(e) => {
					if (e.key === 'Enter' && !e.shiftKey) {
						e.preventDefault();
						run();
					}
				}}
			/>
		</InputGroup.Root>
		<Button type="submit" disabled={running || !command.trim()}>
			{#if running}<Spinner data-icon="inline-start" />{:else}<PlayIcon data-icon="inline-start" />{/if}
			Esegui
		</Button>
	</form>
	{#if current.examples.length || history.length}
		<div class="flex flex-wrap items-center gap-1.5">
			{#each history.filter((h) => h.kind === kind) as h (h.command)}
				<Button variant="ghost" size="xs" class="block max-w-80 truncate font-mono" onclick={() => (command = h.command)} title={h.command}>{h.command.replace(/\s+/g, ' ')}</Button>
			{/each}
			{#each current.examples.filter((ex) => !history.some((h) => h.kind === kind && h.command === ex)) as ex (ex)}
				<Button variant="outline" size="xs" class="font-mono" onclick={() => (command = ex)}>{ex}</Button>
			{/each}
		</div>
	{/if}
	{#if job}
		{#key job}
			<JobLog id={job} ondone={() => (running = false)} />
		{/key}
	{/if}
</div>
